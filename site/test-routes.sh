#!/usr/bin/env bash
# Checks every redirect, short link, route and header of the site.
#
#   ./test-routes.sh                       # against the live https://getvinter.app etc.
#   ./test-routes.sh --local 127.0.0.1:8080  # against a local nginx/container (sets Host headers)
#
# Exits non-zero if any check fails.
set -uo pipefail

LOCAL=""
if [[ "${1:-}" == "--local" ]]; then LOCAL="${2:?usage: --local host:port}"; fi

pass=0; fail=0
GH=https://github.com/praevit/vinter

# fetch HOST PATH -> sets STATUS, LOCATION, HEADERS, BODY
fetch() {
  local host=$1 path=$2 url
  if [[ -n "$LOCAL" ]]; then url="http://$LOCAL$path"; else url="https://$host$path"; fi
  local tmp err; tmp=$(mktemp); err=$(mktemp)
  HEADERS=$(curl -sS -D - -o "$tmp" -H "Host: $host" --max-time 15 "$url" 2>"$err" | tr -d '\r')
  BODY=$(cat "$tmp")
  STATUS=$(printf '%s\n' "$HEADERS" | head -1 | awk '{print $2}')
  LOCATION=$(printf '%s\n' "$HEADERS" | awk -F': ' 'tolower($1)=="location"{print $2}')
  # No HTTP response at all: say why (DNS, TLS, timeout...).
  if [[ -z "$STATUS" ]]; then STATUS="none"; LOCATION="($(tr -d '\n' <"$err" | sed 's/^curl: //'))"; fi
  rm -f "$tmp" "$err"
}

ok()  { pass=$((pass+1)); printf 'PASS  %s\n' "$1"; }
bad() { fail=$((fail+1)); printf 'FAIL  %s\n      %s\n' "$1" "$2"; }

# expect HOST PATH STATUS [LOCATION]
expect() {
  local host=$1 path=$2 status=$3 location=${4:-}
  fetch "$host" "$path"
  local what="$host$path -> $status${location:+ $location}"
  if [[ "$STATUS" != "$status" ]]; then bad "$what" "got $STATUS ${LOCATION}"; return; fi
  if [[ -n "$location" && "$LOCATION" != "$location" ]]; then bad "$what" "got Location: $LOCATION"; return; fi
  ok "$what"
}

has_header() {
  local what=$1 pattern=$2
  if printf '%s\n' "$HEADERS" | grep -qiE "$pattern"; then ok "$what"; else bad "$what" "missing header matching /$pattern/"; fi
}

echo "== Alias domains keep the path and query"
expect vinterapp.com          /                       301 https://getvinter.app/
expect vinterapp.com          /download               301 https://getvinter.app/download
expect www.vinterapp.com      "/some/path?x=1&y=2"    301 "https://getvinter.app/some/path?x=1&y=2"
expect www.getvinter.app      /                       301 https://getvinter.app/
expect www.getvinter.app      /GitHub/issues          301 https://getvinter.app/GitHub/issues

echo "== Short links: any case, trailing slash, query kept"
for p in /download /download/ /Download /DOWNLOAD/ /download/mac /download/mac/ /Download/Mac /download/linux /download/linux/ /download/anything; do
  expect getvinter.app "$p" 302 "$GH/releases/latest"
done
expect getvinter.app "/download?ref=newsletter"       302 "$GH/releases/latest?ref=newsletter"
expect getvinter.app "/Download/Mac/?ref=x"           302 "$GH/releases/latest?ref=x"

echo "== Short links: sub-paths pass through"
expect getvinter.app /github                          302 "$GH"
expect getvinter.app /github/                         302 "$GH/"
expect getvinter.app /GitHub                          302 "$GH"
expect getvinter.app /github/issues/12                302 "$GH/issues/12"
expect getvinter.app "/github/blob/main/README.md?plain=1" 302 "$GH/blob/main/README.md?plain=1"
expect getvinter.app /issues                          302 "$GH/issues"
expect getvinter.app /Issues/12/                      302 "$GH/issues/12/"
expect getvinter.app /changelog                       302 "$GH/releases"
expect getvinter.app /changelog/tag/v1.0              302 "$GH/releases/tag/v1.0"
expect getvinter.app /contact                         302 "mailto:hello@getvinter.app"
expect getvinter.app /Contact/                        302 "mailto:hello@getvinter.app"
expect getvinter.app "/contact?subject=Hello"         302 "mailto:hello@getvinter.app?subject=Hello"

echo "== Look-alikes are not short links"
expect getvinter.app /downloads                       404
expect getvinter.app /githubx                         404
expect getvinter.app /contacts                        404

echo "== No header injection through the path"
fetch getvinter.app "/github/%0d%0aSet-Cookie:%20pwned=1"
if [[ "$STATUS" == 302 ]] && ! printf '%s\n' "$HEADERS" | grep -qi '^set-cookie'; then
  ok "encoded CRLF stays encoded (Location: $LOCATION)"
else
  bad "encoded CRLF stays encoded" "status $STATUS, headers: $(printf '%s' "$HEADERS" | tr '\n' '|')"
fi

echo "== Pages, health, updates, 404"
expect getvinter.app /                                200
has_header "home is HTML"            '^content-type: text/html'
has_header "home is not cached"      '^cache-control: no-cache'
has_header "CSP"                     "^content-security-policy: default-src 'none'"
has_header "X-Frame-Options DENY"    '^x-frame-options: DENY'
has_header "nosniff"                 '^x-content-type-options: nosniff'
has_header "Referrer-Policy"         '^referrer-policy: strict-origin-when-cross-origin'
has_header "Permissions-Policy"      '^permissions-policy: camera=\(\)'
if printf '%s\n' "$HEADERS" | grep -qiE '^server: nginx/[0-9]'; then bad "server version hidden" "$(printf '%s\n' "$HEADERS" | grep -i '^server')"; else ok "server version hidden"; fi
if [[ -z "$LOCAL" ]]; then :; else
  expect localhost /                                  200
fi
expect getvinter.app /healthz                         200
[[ "$BODY" == "ok" ]] && ok "/healthz says ok" || bad "/healthz says ok" "body: $BODY"
expect getvinter.app "/styles.css?v=1"                200
has_header "CSS cached for a year"   '^cache-control: public, max-age=31536000, immutable'
expect getvinter.app /updates/latest.json             200
has_header "updates not cached"      '^cache-control: no-cache'
expect getvinter.app /nope                            404
[[ "$BODY" == *"went into hibernation"* ]] && ok "custom 404 page" || bad "custom 404 page" "body did not match"
has_header "404 keeps security headers" "^x-frame-options: DENY"

echo
echo "$pass passed, $fail failed"
[[ $fail -eq 0 ]]
