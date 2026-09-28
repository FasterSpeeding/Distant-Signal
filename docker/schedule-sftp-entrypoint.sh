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
# INF-13: the loaddata file holds the push account's password, so only this
# user may read it. The chart puts it on a memory-backed emptyDir
# (SCHEDULE_SFTP_LOADDATA_DIR); local dev keeps /tmp.
LOADDATA_FILE="${SCHEDULE_SFTP_LOADDATA_DIR:-/tmp}/schedule-sftp-loaddata.json"
umask 077

# Every value below goes into a JSON string literal, escaped here: a '"' or
# '\' in the password (or username) must not be able to break out of its
# string and add or change fields. Control characters are rejected outright.
# Keep in step with charts/distant-signal/templates/schedulefeed-configmap.yaml.
reject_control_chars() {
    case "$2" in
        *[[:cntrl:]]*)
            echo "sftp-entrypoint: $1 contains a control character (newline, tab, ...); refusing to provision the account" >&2
            exit 1
            ;;
        *) ;; # no control characters: accept
    esac
}
json_escape() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

reject_control_chars SCHEDULE_SFTP_USERNAME "${SCHEDULE_SFTP_USERNAME}"
reject_control_chars SCHEDULE_SFTP_PASSWORD "${SCHEDULE_SFTP_PASSWORD}"
reject_control_chars SCHEDULE_FEED_DESTINATION_PATH "${HOME_DIR}"
USERNAME_JSON="$(json_escape "${SCHEDULE_SFTP_USERNAME}")"
PASSWORD_JSON="$(json_escape "${SCHEDULE_SFTP_PASSWORD}")"
HOME_DIR_JSON="$(json_escape "${HOME_DIR}")"

cat >"${LOADDATA_FILE}" <<EOF
{
  "version": 17,
  "users": [
    {
      "status": 1,
      "username": "${USERNAME_JSON}",
      "password": "${PASSWORD_JSON}",
      "home_dir": "${HOME_DIR_JSON}",
      "permissions": {
        "/": ["*"]
      }
    }
  ]
}
EOF

exec sftpgo serve --loaddata-from "${LOADDATA_FILE}" --loaddata-mode 1
