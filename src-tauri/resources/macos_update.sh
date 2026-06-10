#!/bin/bash
#
# Magnolia macOS update helper.
#
# This runs as a DETACHED process, spawned by the app right before it exits.
# Because it outlives the app, by the time it touches the .app bundle the old
# binary is no longer running and nothing is locked — that is the whole point
# of doing the swap out here rather than from inside the running app.
#
# Args:
#   $1  path to the downloaded .zip
#   $2  path to the installed .app bundle to replace
#   $3  PID of the (now-exiting) app, so we can wait for it to fully quit
#
# Privilege escalation order when the plain replace can't write the target:
#   1. sudo  (Touch ID via pam_tid, if the user enabled it) — no password typing
#   2. osascript "with administrator privileges" — native dialog, Touch ID on
#      capable Macs, password fallback everywhere else
# Every tool used here ships with base macOS; none require the Xcode CLT.

set -u

ZIP="$1"
TARGET_APP="$2"
APP_PID="${3:-0}"

LOG="${TMPDIR:-/tmp}/magnolia-update.log"
exec >>"$LOG" 2>&1
echo "=== magnolia update $(date) ==="
echo "zip=$ZIP target=$TARGET_APP pid=$APP_PID"

fail() {
    echo "ERROR: $*"
    # Best effort: relaunch whatever is still there so the user isn't left with
    # no app at all.
    [ -d "$TARGET_APP" ] && open "$TARGET_APP"
    exit 1
}

# ── 1. Wait for the app to fully exit (releases the binary) ──────────────────
if [ "$APP_PID" != "0" ]; then
    for _ in $(seq 1 100); do        # up to ~20s
        kill -0 "$APP_PID" 2>/dev/null || break
        sleep 0.2
    done
fi

[ -f "$ZIP" ] || fail "zip not found: $ZIP"
[ -d "$TARGET_APP" ] || fail "target app not found: $TARGET_APP"

# ── 2. Extract into a staging dir (ditto preserves bundle metadata) ──────────
STAGING="$(mktemp -d "${TMPDIR:-/tmp}/magnolia-update.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
echo "extracting to $STAGING"
ditto -x -k "$ZIP" "$STAGING" || fail "ditto extract failed"

NEW_APP="$(find "$STAGING" -maxdepth 3 -name '*.app' -type d | head -n1)"
[ -n "$NEW_APP" ] || fail "no .app found inside zip"
echo "new app: $NEW_APP"

# Strip the quarantine flag so Gatekeeper doesn't block first launch.
xattr -dr com.apple.quarantine "$NEW_APP" 2>/dev/null || true

# ── 3. Replace the installed bundle ─────────────────────────────────────────
# Try unprivileged first — succeeds for the common case of an admin user with a
# normal /Applications, and for anything under the user's home.
replace_plain() {
    rm -rf "$TARGET_APP" 2>/dev/null && \
    ditto "$NEW_APP" "$TARGET_APP" 2>/dev/null
}

touchid_enabled() {
    # pam_tid present and uncommented in either PAM sudo config.
    grep -Eq '^[[:space:]]*auth[[:space:]]+sufficient[[:space:]]+pam_tid\.so' \
        /etc/pam.d/sudo_local /etc/pam.d/sudo 2>/dev/null
}

replace_sudo() {
    # No -n: we WANT pam_tid to show its Touch ID dialog. If Touch ID isn't
    # actually available, sudo exits quickly ("no tty") rather than hanging,
    # and we fall through to the osascript path.
    sudo -p '' rm -rf "$TARGET_APP" && \
    sudo -p '' ditto "$NEW_APP" "$TARGET_APP"
}

replace_osascript() {
    # Double-quotes are escaped for AppleScript's string literal.
    local script="rm -rf '$TARGET_APP' && /usr/bin/ditto '$NEW_APP' '$TARGET_APP'"
    osascript -e "do shell script \"${script//\"/\\\"}\" with administrator privileges"
}

echo "replacing bundle..."
if replace_plain; then
    echo "replaced without elevation"
elif touchid_enabled && replace_sudo; then
    echo "replaced via sudo (Touch ID)"
elif replace_osascript; then
    echo "replaced via osascript admin"
else
    fail "could not replace app bundle"
fi

xattr -dr com.apple.quarantine "$TARGET_APP" 2>/dev/null || true

# ── 4. Relaunch and clean up ────────────────────────────────────────────────
echo "relaunching $TARGET_APP"
open "$TARGET_APP" || fail "relaunch failed"
rm -f "$ZIP" 2>/dev/null || true
echo "update complete"
exit 0
