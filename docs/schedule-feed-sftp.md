# Schedule feed SFTP: push account controls

The schedulefeed pod's `sftp` container (SFTPGo v2.7.5) is the one service on
the node's public IP. DTD (the Rail Data Marketplace push) logs in as
`dtd-push` and uploads the CIF timetable zip, and sometimes the CORPUS
extract. DTD will not pin our host key or publish its source addresses, so an
IP allow-list and host-key pinning are not available. These controls make up
for that.

Code and config:

- `charts/distant-signal/files/schedule-sftp-entrypoint.sh` provisions the
  account through SFTPGo's `--loaddata-from` JSON;
- `charts/distant-signal/templates/schedulefeed-deployment.yaml` wires the
  `scheduleFeed.sftp.*` values into it;
- `crates/schedule-ingest` checks every delivered file.

## Least privilege for `dtd-push`

| Setting | Value | Why |
| --- | --- | --- |
| `permissions` on `/` | `upload`, `overwrite`, `list` | See the evidence below |
| `filters.denied_login_methods` | every method except `password` and `keyboard-interactive` (password mode), or except `publickey` (public-key mode) | DTD logs in with keyboard-interactive, which SFTPGo answers with the account password |
| `filters.denied_protocols` | `FTP`, `DAV`, `HTTP` | SSH/SFTP only. FTP and WebDAV are off server-wide anyway, and the web client with them |
| `max_sessions` | `2` (`scheduleFeed.sftp.maxSessions`) | DTD opens one session per delivery |
| `filters.max_upload_file_size` | 512 MiB (`scheduleFeed.sftp.maxUploadFileSize`) | ~6.6x the largest real file. A larger upload fails and SFTPGo deletes the partial file |

Not granted: `download`, `delete`, `rename`, `create_dirs`, `create_symlinks`,
`chmod`, `chown`, `chtimes`, `copy`. A stolen password can then add or replace
files in the one directory, but can't read, delete, rename or hide anything,
and can't fill the volume with one upload. What it can still do, replace the
timetable with a crafted one, is what schedule-ingest's checks are for
([below](#delivery-checks)).

No quota (`quota_size`) is set: schedule-ingest never deletes the CIF zip, so
SFTPGo's used-quota counter would only grow, and a quota would eventually
block a legitimate delivery.

The SQLite user store is on the container's writable layer, so every
container start creates the account afresh from the loaddata file, and
`--loaddata-mode 0` updates it if it ever already exists. The file holds the
credential, so `--loaddata-clean` deletes it as soon as SFTPGo has loaded it.
The pod template carries a `checksum/sftp-entrypoint` annotation, so a change
to the script rolls the pod.

### Evidence: what DTD's client does

DTD's client identifies as `SSH-2.0-JSCH-0.1.54`. Sources, all read-only on
2026-10-01:

- Loki, `{namespace="distant-signal", container="sftp"}`, 2026-09-30 to
  2026-10-01 (Loki's whole retention at the time). The schedulefeed pod had
  restarted at 16:24Z, so `kubectl logs --since=72h` held nothing older.
- `ls -la --time-style=full-iso` of the PVC from the `ingest` container.

There was one DTD session in that window, on 2026-09-30:

| Time (UTC) | Line | Content |
| --- | --- | --- |
| 19:59:54.927 | `login` | `dtd-push`, method `keyboard-interactive`, client `SSH-2.0-JSCH-0.1.54` |
| 19:59:55.647 | `SFTP` (debug) | transfer added |
| 19:59:59.840 | `Upload` | `/timetable_full.zip`, 77,222,226 bytes, 4,192 ms |
| 19:59:59.875 | `SFTP` | connection closed, exit status 0 |

No `Rename`, `Remove`, `Mkdir`, `Rmdir` or `SetStat` line, all of which
SFTPGo logs at info. So the client uploads straight to the final name: no
temporary name and rename, no mtime preservation, no cleanup.

The PVC shows the same pattern for earlier deliveries. `incoming/`'s own
mtime is 2026-09-28 06:49, unchanged since then, while `timetable_full.zip`
was rewritten on 2026-09-28 20:04:46, 2026-09-29 19:59:01 and 2026-09-30
19:59:59 (the extraction directories' names). A rewrite that leaves the
directory's mtime alone is an in-place overwrite: no entry was created,
deleted or renamed. So `overwrite` is required. Without it the second day's
upload fails, because schedule-ingest reads the zip where it lands and never
removes it.

`list` is kept although no listing was seen. SFTPGo logs neither stat nor
directory reads, and its `Stat`/`Lstat` handlers require `list` on the
parent directory (`internal/sftpd/handler.go`). JSch's `put` stats the
target and treats a failure as "not a directory", so a bare `put` works
without `list`. But JSch's `cd` stats its target and fails without it, and
the log can't show whether DTD's client runs `cd` or `ls` first. Removing it
risks a silent nightly failure; keeping it only lets the account see the
names and sizes of files it delivered itself.

Nothing in the window shows a 16:00 UTC delivery, and the observed time
(~20:00 UTC) is outside schedule-ingest's `checkTimes` overnight window
(22:00 to 01:30 London time). Revisit both once Loki has a month of data.

### Verified against SFTPGo v2.7.5

The release binary (`sftpgo_v2.7.5_linux_x86_64.tar.xz`) was run locally with
this entrypoint and a bolt data provider. With OpenSSH's `sftp` as
`dtd-push` over keyboard-interactive:

| Command | Result |
| --- | --- |
| `put small.zip timetable_full.zip`, twice | both succeed (the second overwrites) |
| `ls -l` | succeeds |
| `get timetable_full.zip` | `remote open: Permission denied` |
| `rename timetable_full.zip x.zip` | `Permission denied` |
| `rm timetable_full.zip` | `Permission denied` |
| `mkdir sub` | `Permission denied` |
| `symlink` | `Operation unsupported` |
| `chmod 600` | `remote setstat: Permission denied` |
| `put` of a file over `maxUploadFileSize` | `Failure`; SFTPGo logs "denying write due to space limit" and deletes the partial file |

SFTPGo does not log refused operations, only the transfers and commands that
succeed, so a probe of these permissions leaves no trace beyond the `login`
line.

The ingest container does not use SFTP. It reads, moves and deletes files on
the shared PVC directly (as group 1000 through the pod's `fsGroup`), so none
of these permissions affect it.

### If DTD's deliveries break

The `ingest` container logs "no .zip delivery observed" after the day's
final check time, and `DistantSignalScheduleReferencePublishStale` fires. To
widen the account without a code change, add the permission to
`scheduleFeed.sftp.permissions` (for example `rename`, if DTD switches to
uploading under a temporary name); the pod restarts with the new policy.
