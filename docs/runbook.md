# Wordfall runbook

For whoever operates the production instance. Everything here follows
PLAN.md § Deployment and Operations and § Backups; where this page and the
plan disagree, the plan wins and this page is wrong.

## One-time setup

These steps need an AWS account, DNS for the domain and GitHub repository
settings. They are done by hand, once, by an operator with admin credentials.

1. **Terraform state.** Create an S3 bucket (versioned) and a DynamoDB lock
   table for state, then:

   ```sh
   cd infra
   terraform init -backend-config="bucket=<state-bucket>" -backend-config="key=wordfall/production.tfstate" \
                  -backend-config="region=<region>" -backend-config="dynamodb_table=<lock-table>"
   ```

2. **Images for the first apply.** The task definitions need image URIs.
   Create the repositories first, then push one build of each:

   ```sh
   terraform apply -target=aws_ecr_repository.backend -target=aws_ecr_repository.frontend \
     -var domain_name=<domain> -var backend_image=x -var frontend_image=x
   # docker build/push as .github/workflows/deploy.yml does, tag e.g. 1-<sha>
   ```

3. **Apply everything.**

   ```sh
   terraform apply -var domain_name=<domain> -var alarm_email=<ops address> \
     -var backend_image=<ecr>/wordfall-backend:<tag> -var frontend_image=<ecr>/wordfall-frontend:<tag>
   ```

   `backup_region` defaults to `us-west-2` and must differ from `region`. The
   apply waits on certificate validation, so run step 4 while it waits.

4. **DNS**, from the outputs:
   - `acm_validation_records`: the CNAMEs that validate the certificate.
   - `ses_dkim_tokens`: three CNAMEs, `<token>._domainkey.<domain>` →
     `<token>.dkim.amazonses.com`.
   - The domain itself: an alias (or CNAME) to `alb_dns_name`.

5. **SES production access.** A new account's SES is in the sandbox and only
   sends to verified addresses. Request production access for the region
   before real users register.

6. **Database roles and secrets.** RDS keeps the master password in Secrets
   Manager (`manage_master_user_password`). Connect as the master user (ECS
   Exec into a task, below) and create the two roles the app and the dump use:

   ```sql
   CREATE ROLE wordfall_app LOGIN PASSWORD '<generated>';
   GRANT ALL ON DATABASE wordfall TO wordfall_app;
   GRANT ALL ON SCHEMA public TO wordfall_app;
   ALTER SCHEMA public OWNER TO wordfall_app;   -- the backend runs the migration
   CREATE ROLE wordfall_backup LOGIN PASSWORD '<generated>';
   GRANT pg_read_all_data TO wordfall_backup;   -- the nightly dump's read-only role
   ```

   Then set the SSM parameters Terraform created with placeholder values:

   | Parameter | Value |
   |---|---|
   | `/wordfall/production/DATABASE_URL` | `postgres://wordfall_app:<pw>@<db_endpoint>:5432/wordfall?sslmode=require` |
   | `/wordfall/production/BACKUP_DATABASE_URL` | `postgres://wordfall_backup:<pw>@<db_endpoint>:5432/wordfall?sslmode=require` |
   | `/wordfall/production/SESSION_SIGNING_KEY` | 64 hex characters: `openssl rand -hex 32` |

   RDS Postgres 16 refuses unencrypted connections, hence `sslmode=require`.
   Restart the service after setting them:
   `aws ecs update-service --cluster wordfall --service wordfall --force-new-deployment`.

7. **GitHub.** Create an environment named `production` (the deploy role
   trusts only that environment). Add the repository variables `AWS_REGION` and
   `AWS_DEPLOY_ROLE_ARN` (the `deploy_role_arn` output).

8. **First admin and first catalog.** Register through the site, confirm the
   email, then grant admin (below). Upload the licensed catalog at `/admin`
   from your own files: distributions, then lexicons, then leave values. They
   are stored only as rows and never as files. Before offering a catalog, set
   the total index size `/admin` shows against the task memory. Each index
   logs its build time and resident size when built. The whole catalog must
   fit in `task_memory`, 8 GiB by default, with room for exports.

## Deploying

Actions → **Deploy** → Run workflow, on `main`, once CI is green. It builds
and pushes both images, tagged `<run number>-<commit>`. The run number is the
monotonic build number the app sends with every sync. It then registers new
task definition revisions for the app and the nightly dump, and rolls the
service. The backend migrates before it binds, and the ALB sends traffic only
once `/health` is ready, which includes the catalog indexes.

- **Sync compatibility.** A sync API change must stay compatible with the
  previous app version for at least `SYNC_RETENTION_DAYS` (90). To retire old
  builds, raise `min_app_version` with Terraform. Devices below it are
  answered `426` after their operations are applied, and they offer "reload to
  keep syncing".
- **Infrastructure changes** go through `terraform plan`/`apply` by hand. An
  apply leaves the service on the revision the last deploy registered.
- **Task size.** 2 vCPU matches `SEARCH_CONCURRENCY`'s default. If you raise
  `task_cpu`, raise `search_concurrency` with it.
- **Task count.** Rate-limit buckets are in memory, per task, so with two
  tasks every limit is effectively doubled: the login limit becomes twenty
  failures a minute per username. Terraform caps the service at two tasks
  until a shared limiter exists.

## Granting admin

```sh
aws ecs execute-command --cluster wordfall --task <task-id> --container backend --interactive \
  --command "psql \"$DATABASE_URL\""
```

```sql
UPDATE users SET is_admin = true WHERE username = '<username>';
```

No endpoint can set the flag. The user's next request sees it.

## Backups

| What | Where | Kept |
|---|---|---|
| RDS automated backups, point in time | RDS, same region | 30 days |
| Nightly `pg_dump --format=custom` and `--schema-only` | `s3://wordfall-production-backups-<account>/daily/YYYY-MM-DD.*`, second region, versioned, object-locked | 90 days |
| The first-of-month dump | `…/monthly/YYYY-MM.*` | a year |

The dump runs at 03:15 UTC as the `wordfall-backup` Fargate task, which is
`scripts/backup.py` on the backend image with the read-only role. It reports
`Wordfall/Backups` metrics:
- `DumpSucceeded`
- `DumpBytes`
- `DumpSizeRatio`, this dump's size over the previous one's

To check it by hand, list the bucket and read the task's log stream
`backup/…` in `/wordfall/production`.

## Alarms

All go to the `wordfall-alarms` SNS topic, and to `alarm_email` if set.

| Alarm | Means | First step |
|---|---|---|
| `wordfall-no-dump-36h` | No successful dump in 36 hours | Read the last `wordfall-backup` task's log; run the task by hand |
| `wordfall-dump-shrank` | A dump is more than 30% smaller than the one before | Check the database size first. A truncated or partial run leaves a small object, so compare the two dumps' `pg_restore --list` output |
| `wordfall-db-free-storage` | RDS free storage is under the threshold. A full disk stops backups as well as writes | Raise `db_max_allocated_storage`. Large cascades and 300,000-entry word lists are the main growth |
| `wordfall-sync-error-rejection` | A sync operation was rejected as `error`: a database error the rules did not expect | Find `sync operation failed with a database error` in the logs |

The `wordfall-sync` dashboard graphs rejections by type and reason. Some
reasons are what ordinary two-device use produces:
- `stale_attempt`, `stale`, `not_active` and `not_deepest`
- `not_found`, when it follows one of those in the same batch

A rise in any other reason points to a divergence between the Rust and
TypeScript rules.

## Restoring

1. **Restore into a new instance, never over the live one.** Choose one of two
   routes:
   - **From a nightly dump.** Create an empty database, then run:

     ```sh
     ./scripts/restore.py s3://<bucket>/daily/<day>.dump --s3-region <backup_region> \
       --database-url 'postgres://…/wordfall_restored?sslmode=require'
     ```

   - **From point-in-time recovery.** Restore the RDS instance to a new
     identifier in the console or CLI, then run:

     ```sh
     ./scripts/restore.py --bump-only --database-url 'postgres://…@<new endpoint>/wordfall?sslmode=require'
     ```

2. **Check it before any cutover.** Point a staging stack at it with
   `./scripts/dev.py --env DATABASE_URL=…`. Wait for `/health`, which stays
   not-ready until every lexicon and leave value set has been indexed from the
   rows (a few seconds each). Nothing is re-uploaded. Then open a few cascades.
3. **The resync step** is done by `restore.py` every time, and alone with
   `--bump-only`. It sets every user's `sync_seq` and `sync_floor_seq` to
   `sync_seq + 2^32`. Every device's next sync then answers `resync_required`
   and rebuilds from the server, and its unsent operations are pushed first in
   that same request. It is easy to forget and impossible to skip: without it,
   a device that synced after the restore point would silently ignore rows it
   should pull.
4. **Cut over.** Point `DATABASE_URL` at the restored instance and force a new
   deployment. Then tell users what was lost: work pushed after the restore
   point that lived only on the server. Anything still in a device's outbox is
   pushed again and survives.

## The drill

Quarterly. It is not done until the restored instance serves a real cascade.

1. Restore the latest nightly dump into a throwaway database, as in Restoring.
2. Run the end-to-end suite against it:
   `BASE_URL=<staging url> npx playwright test` in `e2e/`, with an admin account.
3. Before the bump, sign a second browser profile in to the staging stack.
   Its cursor must be ahead of the restored server, with unsent grades in its
   outbox. Run the bump, reconnect that profile, and check three things: the
   grades were applied, the device resynced rather than diverging, and no
   operation was rejected as `invalid`.
4. Record the date and the measured restore time below.

| Date | Dump | Restore time | By | Notes |
|---|---|---|---|---|
| | | | | |

## Locally

`scripts/backup.py` and `scripts/restore.py` work against any Wordfall,
including the development stack:

```sh
./scripts/backup.py                                   # the dev stack → ./backups/
./scripts/restore.py backups/wordfall-<day>.dump --project wordfall --replace
```

`--replace` drops the local stack's schema first. It exists only for local
stacks (`--project`), for the snapshot taken before a risky migration edit.
