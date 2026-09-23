# SSM parameters: names only. Values are set out of band and never in state
# after creation.
locals {
  ssm_prefix = "/wordfall/${var.environment}"
  secret_names = {
    DATABASE_URL        = "${local.ssm_prefix}/DATABASE_URL"
    SESSION_SIGNING_KEY = "${local.ssm_prefix}/SESSION_SIGNING_KEY"
  }
}

resource "aws_ssm_parameter" "secret" {
  for_each = local.secret_names
  name     = each.value
  type     = "SecureString"
  value    = "set-out-of-band"
  lifecycle {
    ignore_changes = [value]
  }
}
