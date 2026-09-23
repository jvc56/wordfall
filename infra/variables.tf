variable "region" {
  type    = string
  default = "us-east-1"
}

variable "environment" {
  type    = string
  default = "production"
}

variable "domain_name" {
  description = "The site's domain, e.g. wordfall.example.com. Used for ACM, SES and PUBLIC_URL."
  type        = string
}

variable "mail_from" {
  type    = string
  default = null
}

variable "vpc_cidr" {
  type    = string
  default = "10.40.0.0/16"
}

variable "backend_image" {
  description = "Backend image URI with tag, pushed by the deploy pipeline."
  type        = string
}

variable "frontend_image" {
  description = "Nginx frontend image URI with tag, pushed by the deploy pipeline."
  type        = string
}

variable "task_cpu" {
  description = "2 vCPU, the figure SEARCH_CONCURRENCY defaults to. Raising it means raising SEARCH_CONCURRENCY too."
  type        = number
  default     = 2048
}

variable "task_memory" {
  description = "Sized to the catalog; each index's size is visible in /admin."
  type        = number
  default     = 8192
}

variable "search_concurrency" {
  type    = number
  default = 2
}

variable "desired_count" {
  type    = number
  default = 2
}

variable "db_instance_class" {
  type    = string
  default = "db.t4g.medium"
}

variable "db_allocated_storage" {
  type    = number
  default = 50
}

variable "db_max_allocated_storage" {
  type    = number
  default = 500
}

variable "db_free_storage_alarm_bytes" {
  description = "Free-storage alarm threshold; about 1.8 GB per account at both limits (PLAN.md § Capacity)."
  type        = number
  default     = 10737418240
}

variable "alarm_email" {
  type    = string
  default = null
}

variable "min_app_version" {
  type    = number
  default = 0
}
