# One task definition with two containers, backend and Nginx, at 2 vCPU.
resource "aws_ecs_cluster" "main" {
  name = "wordfall"
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

locals {
  public_url = "https://${var.domain_name}"
  backend_env = {
    BIND_ADDR          = "127.0.0.1:8080"
    SECURE_COOKIES     = "true"
    MAIL_BACKEND       = "ses"
    MAIL_FROM          = coalesce(var.mail_from, "no-reply@${var.domain_name}")
    PUBLIC_URL         = local.public_url
    SEARCH_CONCURRENCY = tostring(var.search_concurrency)
    MIN_APP_VERSION    = tostring(var.min_app_version)
    TRUSTED_PROXY_HOPS = "1"
    AWS_REGION         = var.region
    RUST_LOG           = "info"
  }
  log_config = {
    logDriver = "awslogs"
    options = {
      awslogs-group         = aws_cloudwatch_log_group.app.name
      awslogs-region        = var.region
      awslogs-stream-prefix = "wordfall"
    }
  }
}

resource "aws_ecs_task_definition" "app" {
  family                   = "wordfall"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.task_cpu
  memory                   = var.task_memory
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.task.arn

  container_definitions = jsonencode([
    {
      name         = "backend"
      image        = var.backend_image
      essential    = true
      environment  = [for k, v in local.backend_env : { name = k, value = v }]
      secrets      = [for k, p in aws_ssm_parameter.secret : { name = k, valueFrom = p.arn }]
      portMappings = [{ containerPort = 8080, protocol = "tcp" }]
      healthCheck = {
        command     = ["CMD-SHELL", "curl -fsS http://127.0.0.1:8080/health || exit 1"]
        interval    = 10
        timeout     = 5
        retries     = 3
        startPeriod = 120
      }
      logConfiguration = local.log_config
    },
    {
      name      = "nginx"
      image     = var.frontend_image
      essential = true
      environment = [
        { name = "BACKEND_UPSTREAM", value = "127.0.0.1:8080" },
        { name = "REAL_IP_FROM", value = var.vpc_cidr },
        { name = "HSTS", value = "max-age=31536000" },
      ]
      portMappings     = [{ containerPort = 80, protocol = "tcp" }]
      dependsOn        = [{ containerName = "backend", condition = "HEALTHY" }]
      logConfiguration = local.log_config
    },
  ])
}

resource "aws_ecs_service" "app" {
  name                               = "wordfall"
  cluster                            = aws_ecs_cluster.main.id
  task_definition                    = aws_ecs_task_definition.app.arn
  desired_count                      = var.desired_count
  launch_type                        = "FARGATE"
  health_check_grace_period_seconds  = 300
  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200

  network_configuration {
    subnets          = aws_subnet.private[*].id
    security_groups  = [aws_security_group.tasks.id]
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.web.arn
    container_name   = "nginx"
    container_port   = 80
  }

  depends_on = [aws_lb_listener.https]
}
