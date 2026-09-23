resource "aws_ecr_repository" "backend" {
  name                 = "wordfall-backend"
  image_tag_mutability = "IMMUTABLE"
}

resource "aws_ecr_repository" "frontend" {
  name                 = "wordfall-frontend"
  image_tag_mutability = "IMMUTABLE"
}
