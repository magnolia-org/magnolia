#!/bin/bash
#
# Enable Touch ID authentication for sudo.
#
# This script is the privileged part — it is run via:
#   osascript -e 'do shell script "/bin/bash <this> " with administrator privileges'
# so it already executes as root and never calls sudo itself (avoiding a
# chicken-and-egg problem). The one admin prompt it triggers is the native
# macOS dialog, which itself supports Touch ID on capable Macs and falls back
# to a password everywhere else — so this works on every Mac with no Xcode CLT.
#
# macOS 14 (Sonoma)+ ships /etc/pam.d/sudo_local, a drop-in that survives OS
# updates; we prefer it. Older releases get pam_tid added directly to
# /etc/pam.d/sudo.

set -eu

PAM_LINE='auth       sufficient     pam_tid.so'

already_enabled() {
    grep -Eq '^[[:space:]]*auth[[:space:]]+sufficient[[:space:]]+pam_tid\.so' \
        /etc/pam.d/sudo_local /etc/pam.d/sudo 2>/dev/null
}

if already_enabled; then
    echo "touch id for sudo already enabled"
    exit 0
fi

major="$(sw_vers -productVersion | cut -d. -f1)"

if [ "$major" -ge 14 ] && grep -q 'sudo_local' /etc/pam.d/sudo 2>/dev/null; then
    # Sonoma+: use the drop-in. Seed from the template if present.
    F=/etc/pam.d/sudo_local
    if [ ! -f "$F" ]; then
        if [ -f "$F.template" ]; then
            cp "$F.template" "$F"
        else
            : > "$F"
        fi
    fi
    if ! grep -q 'pam_tid.so' "$F"; then
        printf '%s\n' "$PAM_LINE" >> "$F"
    fi
    chown root:wheel "$F"
    chmod 444 "$F"
    echo "enabled via $F"
else
    # Older macOS: insert pam_tid as the first auth line of /etc/pam.d/sudo.
    F=/etc/pam.d/sudo
    cp "$F" "$F.magnolia.bak"
    # Put our line right after the leading comment block / before existing auth.
    awk -v line="$PAM_LINE" '
        BEGIN { inserted = 0 }
        /^auth/ && !inserted { print line; inserted = 1 }
        { print }
        END { if (!inserted) print line }
    ' "$F.magnolia.bak" > "$F"
    chown root:wheel "$F"
    chmod 444 "$F"
    echo "enabled via $F"
fi

exit 0
