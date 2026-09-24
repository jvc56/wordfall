# Alarms (PLAN.md § Backups → Alarms, § Deployment and Operations →
# Capacity and Monitoring), to an SNS topic with an optional email subscriber.

resource "aws_sns_topic" "alarms" {
  name = "wordfall-alarms"
}

resource "aws_sns_topic_subscription" "alarm_email" {
  count     = var.alarm_email == null ? 0 : 1
  topic_arn = aws_sns_topic.alarms.arn
  protocol  = "email"
  endpoint  = var.alarm_email
}

# No successful dump in 36 hours: six six-hour periods with no success,
# missing data counting as a miss. scripts/backup.py reports DumpSucceeded.
resource "aws_cloudwatch_metric_alarm" "backup_missing" {
  alarm_name          = "wordfall-no-dump-36h"
  alarm_description   = "No successful nightly dump in 36 hours (PLAN.md § Backups)."
  namespace           = "Wordfall/Backups"
  metric_name         = "DumpSucceeded"
  statistic           = "Sum"
  period              = 21600
  evaluation_periods  = 6
  datapoints_to_alarm = 6
  threshold           = 1
  comparison_operator = "LessThanThreshold"
  treat_missing_data  = "breaching"
  alarm_actions       = [aws_sns_topic.alarms.arn]
  ok_actions          = [aws_sns_topic.alarms.arn]
}

# A dump more than 30% smaller than the previous one: scripts/backup.py
# reports DumpSizeRatio, this dump's bytes over the previous dump's.
resource "aws_cloudwatch_metric_alarm" "backup_shrank" {
  alarm_name          = "wordfall-dump-shrank"
  alarm_description   = "A dump more than 30% smaller than the previous one: a truncated or partial run?"
  namespace           = "Wordfall/Backups"
  metric_name         = "DumpSizeRatio"
  statistic           = "Minimum"
  period              = 86400
  evaluation_periods  = 1
  threshold           = 0.7
  comparison_operator = "LessThanThreshold"
  treat_missing_data  = "notBreaching"
  alarm_actions       = [aws_sns_topic.alarms.arn]
}

# RDS free storage, since a full disk stops backups as well as writes.
resource "aws_cloudwatch_metric_alarm" "db_free_storage" {
  alarm_name          = "wordfall-db-free-storage"
  alarm_description   = "RDS free storage below the threshold (PLAN.md § Capacity)."
  namespace           = "AWS/RDS"
  metric_name         = "FreeStorageSpace"
  dimensions          = { DBInstanceIdentifier = aws_db_instance.main.identifier }
  statistic           = "Minimum"
  period              = 300
  evaluation_periods  = 3
  threshold           = var.db_free_storage_alarm_bytes
  comparison_operator = "LessThanThreshold"
  alarm_actions       = [aws_sns_topic.alarms.arn]
  ok_actions          = [aws_sns_topic.alarms.arn]
}

# Sync rejections, counted by type and reason from the backend's JSON log
# line "sync operation rejected", and graphed. Any `error` rejection alarms:
# a database error the rules did not expect.
resource "aws_cloudwatch_log_metric_filter" "sync_rejections" {
  name           = "wordfall-sync-rejections"
  log_group_name = aws_cloudwatch_log_group.app.name
  pattern        = "{ $.fields.message = \"sync operation rejected\" }"
  metric_transformation {
    namespace = "Wordfall/Sync"
    name      = "Rejections"
    value     = "1"
    dimensions = {
      Type   = "$.fields.op_type"
      Reason = "$.fields.reason"
    }
  }
}

resource "aws_cloudwatch_log_metric_filter" "sync_error_rejections" {
  name           = "wordfall-sync-error-rejections"
  log_group_name = aws_cloudwatch_log_group.app.name
  pattern        = "{ ($.fields.message = \"sync operation rejected\") && ($.fields.reason = \"error\") }"
  metric_transformation {
    namespace     = "Wordfall/Sync"
    name          = "ErrorRejections"
    value         = "1"
    default_value = "0"
  }
}

resource "aws_cloudwatch_metric_alarm" "sync_error_rejections" {
  alarm_name          = "wordfall-sync-error-rejection"
  alarm_description   = "A sync operation was rejected as `error`: a database error the rules did not expect."
  namespace           = "Wordfall/Sync"
  metric_name         = "ErrorRejections"
  statistic           = "Sum"
  period              = 300
  evaluation_periods  = 1
  threshold           = 1
  comparison_operator = "GreaterThanOrEqualToThreshold"
  treat_missing_data  = "notBreaching"
  alarm_actions       = [aws_sns_topic.alarms.arn]
}

# The rejection graph. stale_attempt, stale, not_active and not_deepest (and
# the not_found that can follow them) are ordinary two-device use; a rise in
# any other reason points to a divergence between the Rust and TypeScript rules.
resource "aws_cloudwatch_dashboard" "sync" {
  dashboard_name = "wordfall-sync"
  dashboard_body = jsonencode({
    widgets = [
      {
        type   = "metric"
        x      = 0
        y      = 0
        width  = 24
        height = 8
        properties = {
          title   = "Sync rejections by type and reason"
          region  = var.region
          stat    = "Sum"
          period  = 300
          metrics = [[{ expression = "SEARCH('{Wordfall/Sync,Type,Reason} MetricName=\"Rejections\"', 'Sum', 300)", id = "r" }]]
        }
      },
    ]
  })
}
