# RDS Postgres 16 with 30-day point-in-time recovery and storage autoscaling.
resource "aws_db_subnet_group" "main" {
  name       = "wordfall"
  subnet_ids = aws_subnet.private[*].id
}

resource "aws_db_instance" "main" {
  identifier                   = "wordfall"
  engine                       = "postgres"
  engine_version               = "16"
  instance_class               = var.db_instance_class
  allocated_storage            = var.db_allocated_storage
  max_allocated_storage        = var.db_max_allocated_storage
  storage_type                 = "gp3"
  storage_encrypted            = true
  db_name                      = "wordfall"
  username                     = "wordfall"
  manage_master_user_password  = true
  db_subnet_group_name         = aws_db_subnet_group.main.name
  vpc_security_group_ids       = [aws_security_group.db.id]
  backup_retention_period      = 30
  copy_tags_to_snapshot        = true
  deletion_protection          = true
  skip_final_snapshot          = false
  final_snapshot_identifier    = "wordfall-final"
  performance_insights_enabled = true
  auto_minor_version_upgrade   = true
}
