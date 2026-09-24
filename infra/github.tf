# The deploy workflow's role (.github/workflows/deploy.yml), assumed through
# GitHub's OIDC provider from the main branch only. It can push the two images
# and roll the service onto a new task definition revision, and nothing else:
# infrastructure changes are applied with Terraform by hand (docs/runbook.md).

variable "github_repository" {
  description = "owner/name of the repository whose main branch may deploy."
  type        = string
  default     = "jvc56/wordfall"
}

resource "aws_iam_openid_connect_provider" "github" {
  url             = "https://token.actions.githubusercontent.com"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = ["6938fd4d98bab03faadb97b34396831e3780aea1"]
}

data "aws_iam_policy_document" "deploy_assume" {
  statement {
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [aws_iam_openid_connect_provider.github.arn]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values   = ["repo:${var.github_repository}:environment:production"]
    }
  }
}

resource "aws_iam_role" "deploy" {
  name               = "wordfall-deploy"
  assume_role_policy = data.aws_iam_policy_document.deploy_assume.json
}

data "aws_iam_policy_document" "deploy" {
  statement {
    actions   = ["ecr:GetAuthorizationToken"]
    resources = ["*"]
  }
  statement {
    actions = [
      "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:CompleteLayerUpload",
      "ecr:InitiateLayerUpload", "ecr:PutImage", "ecr:UploadLayerPart",
    ]
    resources = [aws_ecr_repository.backend.arn, aws_ecr_repository.frontend.arn]
  }
  statement {
    actions   = ["ecs:DescribeTaskDefinition", "ecs:RegisterTaskDefinition", "ecs:DescribeServices"]
    resources = ["*"]
  }
  statement {
    actions   = ["ecs:UpdateService"]
    resources = [aws_ecs_service.app.id]
  }
  statement {
    actions   = ["iam:PassRole"]
    resources = [aws_iam_role.execution.arn, aws_iam_role.task.arn, aws_iam_role.backup_task.arn]
  }
}

resource "aws_iam_role_policy" "deploy" {
  name   = "deploy"
  role   = aws_iam_role.deploy.id
  policy = data.aws_iam_policy_document.deploy.json
}

output "deploy_role_arn" {
  description = "The AWS_DEPLOY_ROLE_ARN repository variable for the deploy workflow."
  value       = aws_iam_role.deploy.arn
}
