#!/usr/bin/env bash
set -euo pipefail
APP="${SOAPUI_APP:-/Applications/SoapUI-5.9.1.app/Contents}"
TMP_HOME="$(mktemp -d "${TMPDIR:-/tmp}/soapui-home.XXXXXX")"
"$APP/PlugIns/jre.bundle/Contents/Home/bin/java" -Djava.awt.headless=true -Duser.home="$TMP_HOME" \
  -cp "$APP/java/app/bin/soapui-5.9.1.jar:$APP/java/app/lib/*" \
  groovy.ui.GroovyMain "$(dirname "$0")/oracle.groovy" "$1"
