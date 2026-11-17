# Aule quality runner

One GitHub Actions runner on `aule-server`, shared by Capricorn SW and FW. It runs
Ruff, Clippy, dependency checks and Cppcheck. Results are attached to Actions runs;
SARIF reports from the default branches also appear in each repository's
**Security → Code scanning**. Public pull requests use GitHub-hosted runners.

The organization runner group must match `runner-group.json`: access is limited
to these two repositories and their `quality.yml` workflows at `main` (SW) and
`capricorn` (FW), plus SW's manual runner smoke test in `ci.yml` on `main`.
Do not enable the repository variable until GitHub returns
`restricted_to_workflows: true` and the exact three allowed workflow refs.

## Prepare

Build `Dockerfile` as `aule-quality:20261117` and record its image ID with the
reviewed source commit. The container has a 4 CPU / 6 GiB limit, no published ports,
no host directories or Docker socket, and no host privileges. Only the quality
jobs use it; application deployment and firmware image builds keep their existing
workflows.

GitHub's CLI needs organization runner administration permission to create the
group and request a registration token. A classic OAuth token uses `admin:org`;
a fine-grained token can instead grant only organization self-hosted runner
administration. The administrative token stays on the operator's Mac.

## Activate

1. Apply the reviewed runner group policy through the organization Actions API
   and read it back. If GitHub cannot enforce the workflow restrictions, stop.
2. Merge the reviewed quality workflows into each repository's default branch.
3. Request a short-lived organization registration token. Supply it as
   `RUNNER_REGISTRATION_TOKEN` when running `docker compose up -d` here.
   Never commit the token or save it in a repository `.env` file.
4. Verify `aule-quality` is online in the restricted group. Set repository
   variable `AULE_QUALITY_RUNNER=aule-quality` in both repositories.
5. Dispatch each Code quality workflow and verify its runner name and reports.

Without that repository variable, all quality jobs use `ubuntu-latest`, so a
pending runner installation does not strand CI jobs in the queue.

## Operations

The runner's registration and caches live in the container's writable layer.
Restarting it preserves them; recreating it requires a fresh registration token
and removal of the old offline runner record. The runner can update its own
binaries; Dependabot also tracks the base image tag for reproducible rebuilds.

To stop using the server, delete `AULE_QUALITY_RUNNER` in both repositories, then
stop this Compose project. Keep the runner group policy intact until its runner
has been removed. Do not stop, prune or recreate the application's containers.

FW's Cppcheck pass covers the Capricorn board source without generated ESP-IDF
headers. It complements the existing three-board compiler matrix; it does not
validate runtime behavior or hardware. Dependabot does not support ESP-IDF's
component registry; those components remain aligned with XiaoZhi upstream.
