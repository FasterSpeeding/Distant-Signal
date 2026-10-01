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

## Brute force and abuse

Server-wide SFTPGo settings, all on by default (`scheduleFeed.sftp.*`):

- **SSH commands off** (`sshCommands: []`). The image enables `md5sum`,
  `sha1sum`, `sha256sum`, `cd`, `pwd` and `scp`; DTD uses only the SFTP
  subsystem. Verified: `ssh dtd-push@... sha256sum` and `scp` both get
  "exec request failed".
- **Defender** (`defender.*`): a source IP is banned for 60 minutes (longer
  each time it comes back) once its score reaches 8 in 30 minutes. A wrong
  password for `dtd-push` scores 2 per attempt, and OpenSSH and JSch each make
  two attempts per connection, so about two bad connections ban a source.
  Verified locally: the second wrong-password connection logged
  `"sender":"defender","event":"banned"`, and the correct password from the
  same IP was then refused ("connection refused, ip ... is banned").
  Connections that never authenticate (the kubelet's TCP probe, scanners)
  score 0. Bans are in memory and a pod restart clears them.
- **Per-host cap** of 8 simultaneous connections, and a **per-source rate
  limit** of 20 connections a minute (burst 10); hitting either scores 4.
- `defender.safelist` exempts given IPs/CIDRs from both. Empty: DTD's
  addresses are unknown.

The defender needs real client addresses
(`scheduleFeed.service.externalTrafficPolicy: Local`). Behind source NAT every
client shares one address, and a ban would lock DTD out too.

## Audit log

SFTPGo writes one JSON object per line to stdout, which Alloy ships to Loki
as `{namespace="distant-signal", container="sftp"}`. The chart pins the level
to `debug` (`scheduleFeed.sftp.logLevel`), because failed logins and defender
scores are only logged at debug, and sets `SFTPGO_LOG_UTC_TIME=true`, so the
zone-less `time` field is UTC. Every line has `level` (lowercase), `time`
(`2006-01-02T15:04:05.000`, UTC) and `sender`. Lines below are from the local
SFTPGo 2.7.5 run (IPs are loopback there; in production they are the client's
real address, since `externalTrafficPolicy: Local`).

| `sender` | Level | Fields | Meaning |
| --- | --- | --- | --- |
| `login` | info | `ip`, `username`, `method` (`keyboard-interactive`, `password`, `publickey`), `protocol` (`SSH`), `connection_id`, `client` (SSH version string, DTD's is `SSH-2.0-JSCH-0.1.54`), `encrypted`, `info` (negotiated algorithms) | A successful login |
| `connection_failed` | debug | `client_ip`, `username` (empty if none was tried), `login_type` (`keyboard-interactive`, `password`, `publickey`, `no_auth_tried`), `protocol`, `error` (`invalid credentials`; `not found: username "x" does not exist`) | A failed login. `no_auth_tried` is a connection that never authenticated: the kubelet's TCP probe (every 30s from the pod network) and port scanners |
| `Upload` | info, or error when it failed | `remote_addr` (`ip:port`), `local_addr`, `username`, `file_path`, `virtual_path`, `size_bytes`, `elapsed_ms`, `connection_id` (`SFTP_<login connection_id>_<n>`), `protocol` (`SFTP`), `error` (only on failure) | A completed or failed upload |
| `Rename`, `Remove`, `Mkdir`, `Rmdir`, `SetStat` | info | `remote_addr`, `username`, `file_path`, `target_path`, `connection_id`, ... | A filesystem command that succeeded. `dtd-push` has no permission for any of them, so one appearing means the policy changed |
| `defender` | debug (score), info (ban) | `client_ip`, `protocol`, `event` (`LoginFailed`, `UserNotFound`, `NoLoginTried`, `LimitExceeded`; `banned`), `increase_score_by`, `score` | Defender scoring and bans |

Examples:

```json
{"level":"info","time":"2026-10-01T16:53:57.743","sender":"login","ip":"127.0.0.1","username":"dtd-push","method":"keyboard-interactive","protocol":"SSH","connection_id":"adb66d8d…","client":"SSH-2.0-OpenSSH_10.2","encrypted":true,"info":"negotiated algorithms: {...}"}
{"level":"info","time":"2026-10-01T16:53:57.751","sender":"Upload","local_addr":"127.0.0.1:2299","remote_addr":"127.0.0.1:37628","elapsed_ms":0,"size_bytes":4000,"username":"dtd-push","file_path":"/data/schedule-feed/incoming/timetable_full.zip","virtual_path":"/timetable_full.zip","connection_id":"SFTP_adb66d8d…_1","protocol":"SFTP"}
{"level":"error","time":"2026-10-01T16:53:57.880","sender":"Upload","local_addr":"127.0.0.1:2299","remote_addr":"127.0.0.1:37636","elapsed_ms":15,"size_bytes":0,"username":"dtd-push","file_path":"/data/schedule-feed/incoming/big.zip","virtual_path":"/big.zip","connection_id":"SFTP_cd88a96e…_1","protocol":"SFTP","error":"failure: denying write due to space limit"}
{"level":"debug","time":"2026-10-01T16:53:58.052","sender":"connection_failed","client_ip":"127.0.0.1","username":"dtd-push","login_type":"keyboard-interactive","protocol":"SSH","error":"invalid credentials"}
{"level":"debug","time":"2026-10-01T16:54:01.437","sender":"connection_failed","client_ip":"127.0.0.1","username":"scanner","login_type":"keyboard-interactive","protocol":"SSH","error":"not found: username \"scanner\" does not exist"}
{"level":"debug","time":"2026-10-01T16:54:01.437","sender":"defender","client_ip":"127.0.0.1","protocol":"SSH","event":"UserNotFound","increase_score_by":2,"score":8}
{"level":"info","time":"2026-10-01T16:54:01.437","sender":"defender","client_ip":"127.0.0.1","protocol":"SSH","event":"banned"}
```

Note the field names differ by line: the client address is `ip` on `login`,
`client_ip` on `connection_failed` and `defender`, and `remote_addr` (with the
port) on `Upload`. SFTPGo never logs passwords or keyboard-interactive
answers, and does not log a refused operation (a `get` or `rm` the account
has no permission for), only the ones that succeed.

schedule-ingest adds one line per delivered file (`target` =
`schedule_ingest::audit`, see [delivery checks](#delivery-checks)). Join it
to the `Upload` line on file name, size and time: ingest's `delivered_at` is
the file's mtime, which SFTPGo sets when the upload closes.

### LogQL for login anomalies

For the Loki ruler (Ranma owns the rules). The selectors below use the raw
container stream; once Alloy tags the audit lines, `{audit="sftp-delivery"}`
is the cheaper selector for the login lines.

Successful `dtd-push` logins, as a table:

```logql
{namespace="distant-signal", container="sftp"} |= `"sender":"login"`
  | json username, ip, method, client
  | username = "dtd-push"
```

A successful `dtd-push` login from an IP not seen in the previous 30 days
(needs 30 days of retention for these lines, and a `max_query_length` over
30 days):

```logql
sum by (ip) (count_over_time(
  {namespace="distant-signal", container="sftp"} |= `"sender":"login"`
    | json username, ip | username = "dtd-push" [10m]))
unless on (ip)
sum by (ip) (count_over_time(
  {namespace="distant-signal", container="sftp"} |= `"sender":"login"`
    | json username, ip | username = "dtd-push" [30d] offset 10m))
```

A successful `dtd-push` login outside the expected windows. LogQL has no
hour function, so this matches on the UTC `time` field; label regexes are
anchored. For 22:00 to 01:29 and 16:00 to 16:59 UTC:

```logql
sum by (ip) (count_over_time(
  {namespace="distant-signal", container="sftp"} |= `"sender":"login"`
    | json username, ip, time
    | username = "dtd-push"
    | time !~ `.*T(22|23|00):.*|.*T01:[0-2].*|.*T16:.*` [10m])) > 0
```

The deliveries actually seen came at about 20:00 UTC (19:59 and 20:04), which
those windows would flag. Until a month of logins shows DTD's real pattern,
the observed window is the safer one: `time !~ ".*T(19|20):.*"`.

Failed logins that tried a password (excluding probes):

```logql
{namespace="distant-signal", container="sftp"} |= `"sender":"connection_failed"`
  | json client_ip, username, login_type, error
  | login_type != "no_auth_tried"
```

Bans: `{namespace="distant-signal", container="sftp"} |= "\"event\":\"banned\""`.

## Telemetry and alerts

`scheduleFeed.sftp.telemetry` (on with `metrics.enabled`) serves SFTPGo's
`/metrics` and `/healthz` on port 9097, named `sftp-metrics`, scraped by the
chart's PodMonitor and admitted only from the monitoring namespace. It is not
on the NodePort Service. The counters are global, with no username, IP or
protocol labels, so login anomalies come from the log, not the metrics.

### Alerts

Group `distant-signal.schedule-sftp` (`metrics.prometheusRule.scheduleSftp`):

| Alert | Fires when | What to do |
| --- | --- | --- |
| `DistantSignalSftpNoUpload` | no upload in 30h | Check the `login`/`Upload` lines for DTD's last session; if none, DTD did not push. A CORPUS upload also counts, so `DistantSignalScheduleReferencePublishStale` is the timetable signal |
| `DistantSignalSftpUploadErrors` | an upload failed or was cut off in the last hour | Find the `Upload` line with `"level":"error"` and its `error`; check that the next ingest went through |
| `DistantSignalSftpUserStoreDown` | SFTPGo's user store is down for 5m | Every login fails. Restart the pod; the store is rebuilt from the entrypoint at start |

## Host keys

SFTPGo serves the chart's preserved ECDSA (P-256, for JSch 0.1.54) and
ed25519 host keys (`scheduleFeed.sftp.hostKeys`); there is no RSA key. DTD
does not pin them, so the fingerprints are for information only. They can't
be derived without reading the private keys from the Secret, so read them
from SFTPGo's startup log after a deploy:

```logql
{namespace="distant-signal", container="sftp"} |= `"sender":"sftpd"` |= `fingerprint`
```

(`Host key "/srv/sftpgo/host_keys/ssh_host_ecdsa_key" loaded, type
"ecdsa-sha2-nistp256", fingerprint "SHA256:..."`), or from outside with
`ssh-keyscan -p 30450 <host> | ssh-keygen -lf -`.

| Key | Fingerprint |
| --- | --- |
| `ecdsa-sha2-nistp256` | not yet recorded: the running pod (2026-10-01) still generates throwaway keys; record after the first deploy that serves the preserved ones |
| `ssh-ed25519` | as above |

## If DTD's deliveries break

The `ingest` container logs "no .zip delivery observed" after the day's
final check time, and `DistantSignalScheduleReferencePublishStale` fires. To
widen the account without a code change, add the permission to
`scheduleFeed.sftp.permissions` (for example `rename`, if DTD switches to
uploading under a temporary name); the pod restarts with the new policy.
