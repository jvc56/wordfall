data "aws_iam_policy_document" "ecs_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "execution" {
  name               = "wordfall-execution"
  assume_role_policy = data.aws_iam_policy_document.ecs_assume.json
}

resource "aws_iam_role_policy_attachment" "execution" {
  role       = aws_iam_role.execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

data "aws_iam_policy_document" "execution_ssm" {
  statement {
    actions   = ["ssm:GetParameters"]
    resources = [for p in aws_ssm_parameter.secret : p.arn]
  }
}

resource "aws_iam_role_policy" "execution_ssm" {
  name   = "ssm"
  role   = aws_iam_role.execution.id
  policy = data.aws_iam_policy_document.execution_ssm.json
}

resource "aws_iam_role" "task" {
  name               = "wordfall-task"
  assume_role_policy = data.aws_iam_policy_document.ecs_assume.json
}

data "aws_iam_policy_document" "task_ses" {
  statement {
    actions   = ["ses:SendEmail"]
    resources = [aws_sesv2_email_identity.domain.arn]
  }
}

resource "aws_iam_role_policy" "task_ses" {
  name   = "ses"
  role   = aws_iam_role.task.id
  policy = data.aws_iam_policy_document.task_ses.json
}

# ECS Exec sessions into the backend container (docs/runbook.md: granting admin).
data "aws_iam_policy_document" "task_exec" {
  statement {
    actions = [
      "ssmmessages:CreateControlChannel", "ssmmessages:CreateDataChannel",
      "ssmmessages:OpenControlChannel", "ssmmessages:OpenDataChannel",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_role_policy" "task_exec" {
  name   = "ecs-exec"
  role   = aws_iam_role.task.id
  policy = data.aws_iam_policy_document.task_exec.json
}
