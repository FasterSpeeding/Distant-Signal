#!/bin/sh
# Provisions the actual SFTP-login account DTD's push client authenticates
# as, for the local-dev `schedule-sftp` service in docker-compose.yml.
#
# SFTPGO_DEFAULT_ADMIN_USERNAME/PASSWORD (this repo's earlier approach, now
# removed) never worked for this: confirmed directly against SFTPGo's own
# Go source (github.com/drakkan/sftpgo, internal/dataprovider/admin.go's
# Admin.setFromEnv(), called only from dataprovider.go's checkDefaultAdmin())
# that those env vars bootstrap SFTPGo's web-UI/REST-API *admin* account
# (dataprovider.Admin, PermAdminAny) -- a completely separate entity from an
# SFTP-login *user* (dataprovider.User). Setting them never created the
# account DTD (or this service's own paramiko-based test) tries to log in
# as, which is exactly the "not found: sql: no rows in result set" error
# this task started from.
#
# The real, documented mechanism is SFTPGo's own `--loaddata-from` flag /
# SFTPGO_LOADDATA_FROM env var: it loads a JSON dump (same shape as its
# `dumpdata` REST endpoint produces) of users/folders/admins/etc at startup.
# Confirmed directly against SFTPGo's source for this task (checked against
# the `main` branch, i.e. whatever `drakkan/sftpgo:latest` currently builds
# from):
#   - internal/cmd/root.go wires SFTPGO_LOADDATA_FROM/_MODE/_CLEAN/_SCAN as
#     real, current flags (loaddata-mode defaults to 1: "new users are
#     added, existing users are not modified").
#   - internal/service/service.go's Service.Start() -> startServices() calls
#     Service.LoadInitialData() (which reads/parses/restores this file)
#     BEFORE binding the SFTP/FTP/HTTP listeners -- so the account exists
#     before anything can connect, and this needs no HTTP/API port exposed
#     at all.
#   - internal/httpd/api_maintenance.go's RestoreUsers does a
#     username-keyed upsert: with mode 1, a user that already exists from a
#     prior run is left untouched -- so re-running this on every container
#     restart is safe and never resets an already-changed password.
#   - internal/dataprovider/dataprovider.go's createUserPasswordHash (run
#     for every Add/UpdateUser, including the loaddata restore path) hashes
#     any password that isn't already in a recognized hash format -- so the
#     plaintext password below is correct as-is, not a placeholder for a
#     hash this script was supposed to compute itself.
#
# See charts/distant-signal/templates/schedulefeed-deployment.yaml's own
# copy of this reasoning (and its schedulefeed-configmap.yaml's Helm-side
# equivalent of this exact script) for the Kubernetes side.
set -eu

: "${SCHEDULE_SFTP_USERNAME:?SCHEDULE_SFTP_USERNAME must be set}"
: "${SCHEDULE_SFTP_PASSWORD:?SCHEDULE_SFTP_PASSWORD must be set}"

# SCHEDULE_FEED_DESTINATION_PATH matches docker-compose.yml's
# schedule-ingest service's own WATCH_DIR computation exactly (same
# variable, same default) -- this account's home_dir IS its SFTP chroot
# root (SFTPGo's own local-filesystem-provider behaviour: a user with no
# virtual folders is confined to home_dir), set here to the SAME absolute
# path schedule-ingest watches, so DTD's push client -- which will see
# itself uploading to "/" once connected, since home_dir already points at
# the destination -- lands files exactly where schedule-ingest looks for
# them, with no separate subfolder-within-home-dir step for either side to
# get out of sync on. SFTPGo creates home_dir itself if it doesn't already
# exist (vfs/osfs.go's CheckRootPath), so nothing needs to pre-create it.
HOME_DIR="/data/schedule-feed/${SCHEDULE_FEED_DESTINATION_PATH:-incoming}"
LOADDATA_FILE="/tmp/schedule-sftp-loaddata.json"

# NOTE: no JSON-escaping is done on the username/password below. Fine for
# this file's actual values (an operator-chosen username and either the
# Helm chart's own randAlphaNum-generated password or a hand-set local-dev
# default -- none of which plausibly contain '"' or '\'), but a real gap if
# an operator ever sets SCHEDULE_SFTP_PASSWORD to something containing
# those characters; flagged here rather than silently assumed safe.
cat > "$LOADDATA_FILE" <<EOF
{
  "version": 17,
  "users": [
    {
      "status": 1,
      "username": "${SCHEDULE_SFTP_USERNAME}",
      "password": "${SCHEDULE_SFTP_PASSWORD}",
      "home_dir": "${HOME_DIR}",
      "permissions": {
        "/": ["*"]
      }
    }
  ]
}
EOF

exec sftpgo serve --loaddata-from "$LOADDATA_FILE" --loaddata-mode 1
