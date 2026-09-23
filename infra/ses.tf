# SES domain identity for confirmation and reset email.
resource "aws_sesv2_email_identity" "domain" {
  email_identity = var.domain_name
}
