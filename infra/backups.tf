# Backups (PLAN.md § Backups). RDS point-in-time recovery is in rds.tf; this
# is the nightly dump: `pg_dump --format=custom` plus `--schema-only`, run by
# scripts/backup.py as a scheduled Fargate task on the backend image with a
# read-only database role, into an encrypted, versioned, object-locked bucket
# in a second region. Daily dumps are kept 90 days, monthly dumps a year.

provider "aws" {
  alias  = "backup"
  region = var.backup_region
  default_tags {
    tags = { Project = "wordfall", Environment = var.environment }
  }
}

data "aws_caller_identity" "current" {}

resource "aws_s3_bucket" "backups" {
  provider            = aws.backup
  bucket              = "wordfall-${var.environment}-backups-${data.aws_caller_identity.current.account_id}"
  object_lock_enabled = true
}

resource "aws_s3_bucket_versioning" "backups" {
  provider = aws.backup
  bucket   = aws_s3_bucket.backups.id
  versioning_configuration {
    status = "Enabled"
  }
}

# Neither a mistake nor a compromised task role can remove a dump early: the
# task role has no s3:BypassGovernanceRetention, and each key is named for its
# day, so a bad run cannot overwrite a good one either.
resource "aws_s3_bucket_object_lock_configuration" "backups" {
  provider = aws.backup
  bucket   = aws_s3_bucket.backups.id
  rule {
    default_retention {
      mode = "GOVERNANCE"
      days = 90
    }
  }
  depends_on = [aws_s3_bucket_versioning.backups]
}

resource "aws_s3_bucket_server_side_encryption_configuration" "backups" {
  provider = aws.backup
  bucket   = aws_s3_bucket.backups.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "aws:kms"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "backups" {
  provider                = aws.backup
  bucket                  = aws_s3_bucket.backups.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_lifecycle_configuration" "backups" {
  provider = aws.backup
  bucket   = aws_s3_bucket.backups.id
  rule {
    id     = "daily"
    status = "Enabled"
    filter {
      prefix = "daily/"
    }
    expiration {
      days = 91
    }
    noncurrent_version_expiration {
      noncurrent_days = 91
    }
  }
  rule {
    id     = "monthly"
    status = "Enabled"
    filter {
      prefix = "monthly/"
    }
    expiration {
      days = 366
    }
    noncurrent_version_expiration {
      noncurrent_days = 366
    }
  }
  depends_on = [aws_s3_bucket_versioning.backups]
}

# The backup task: the backend image, scripts/backup.py as its entry point,
# the read-only role's DATABASE_URL from SSM (set out of band; see the runbook).
resource "aws_iam_role" "backup_task" {
  name               = "wordfall-backup-task"
  assume_role_policy = data.aws_iam_policy_document.ecs_assume.json
}

data "aws_iam_policy_document" "backup_task" {
  statement {
    actions   = ["s3:PutObject", "s3:PutObjectRetention", "s3:GetObject"]
    resources = ["${aws_s3_bucket.backups.arn}/*"]
  }
  statement {
    actions   = ["s3:ListBucket"]
    resources = [aws_s3_bucket.backups.arn]
  }
  statement {
    actions   = ["cloudwatch:PutMetricData"]
    resources = ["*"]
    condition {
      test     = "StringEquals"
      variable = "cloudwatch:namespace"
      values   = ["Wordfall/Backups"]
    }
  }
}

resource "aws_iam_role_policy" "backup_task" {
  name   = "backup"
  role   = aws_iam_role.backup_task.id
  policy = data.aws_iam_policy_document.backup_task.json
}

resource "aws_ecs_task_definition" "backup" {
  family                   = "wordfall-backup"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = 1024
  memory                   = 2048
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.backup_task.arn

  container_definitions = jsonencode([
    {
      name       = "backup"
      image      = var.backend_image
      essential  = true
      entryPoint = ["python3", "/opt/wordfall/scripts/backup.py"]
      command = [
        "--s3-bucket", aws_s3_bucket.backups.bucket,
        "--s3-region", var.backup_region,
        "--metrics-region", var.region,
      ]
      secrets          = [{ name = "DATABASE_URL", valueFrom = aws_ssm_parameter.secret["BACKUP_DATABASE_URL"].arn }]
      logConfiguration = merge(local.log_config, { options = merge(local.log_config.options, { awslogs-stream-prefix = "backup" }) })
    },
  ])
}

# Nightly, by EventBridge Scheduler.
data "aws_iam_policy_document" "scheduler_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["scheduler.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "backup_scheduler" {
  name               = "wordfall-backup-scheduler"
  assume_role_policy = data.aws_iam_policy_document.scheduler_assume.json
}

data "aws_iam_policy_document" "backup_scheduler" {
  statement {
    actions   = ["ecs:RunTask"]
    resources = [aws_ecs_task_definition.backup.arn_without_revision, "${aws_ecs_task_definition.backup.arn_without_revision}:*"]
  }
  statement {
    actions   = ["iam:PassRole"]
    resources = [aws_iam_role.execution.arn, aws_iam_role.backup_task.arn]
  }
}

resource "aws_iam_role_policy" "backup_scheduler" {
  name   = "run-backup"
  role   = aws_iam_role.backup_scheduler.id
  policy = data.aws_iam_policy_document.backup_scheduler.json
}

resource "aws_scheduler_schedule" "backup" {
  name                         = "wordfall-nightly-dump"
  schedule_expression          = "cron(15 3 * * ? *)"
  schedule_expression_timezone = "UTC"
  flexible_time_window {
    mode = "OFF"
  }
  target {
    arn      = aws_ecs_cluster.main.arn
    role_arn = aws_iam_role.backup_scheduler.arn
    ecs_parameters {
      task_definition_arn = aws_ecs_task_definition.backup.arn_without_revision
      launch_type         = "FARGATE"
      network_configuration {
        subnets          = aws_subnet.private[*].id
        security_groups  = [aws_security_group.tasks.id]
        assign_public_ip = false
      }
    }
    retry_policy {
      maximum_retry_attempts = 2
    }
  }
}
