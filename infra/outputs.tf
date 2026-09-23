output "alb_dns_name" {
  value = aws_lb.main.dns_name
}

output "acm_validation_records" {
  description = "Create these DNS records out of band to validate the certificate."
  value = [for o in aws_acm_certificate.site.domain_validation_options : {
    name = o.resource_record_name, type = o.resource_record_type, value = o.resource_record_value
  }]
}

output "ses_dkim_tokens" {
  value = aws_sesv2_email_identity.domain.dkim_signing_attributes[0].tokens
}

output "ecr_backend" {
  value = aws_ecr_repository.backend.repository_url
}

output "ecr_frontend" {
  value = aws_ecr_repository.frontend.repository_url
}

output "db_endpoint" {
  value = aws_db_instance.main.address
}
