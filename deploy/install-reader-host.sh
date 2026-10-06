#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "run as root" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMIT_SHA="$(tr -d '\r\n' < "${ROOT_DIR}/COMMIT_SHA")"
if [[ ! "${COMMIT_SHA}" =~ ^[0-9a-f]{40}$ ]]; then
  echo "invalid COMMIT_SHA in release packet" >&2
  exit 1
fi

RELEASE="/opt/chaptera/releases/${COMMIT_SHA}"
CURRENT="/opt/chaptera/current"
CONFIG_DIR="/etc/chaptera"
STATE_DIR="/var/lib/chaptera"
CREDENTIAL_DIR="${CONFIG_DIR}/credentials"

if ! id chaptera >/dev/null 2>&1; then
  useradd --system --home "${STATE_DIR}" --shell /usr/sbin/nologin chaptera
fi

install -d -m 0755 /opt/chaptera/releases
install -d -m 0750 -o chaptera -g chaptera "${STATE_DIR}"
install -d -m 0700 -o chaptera -g chaptera "${STATE_DIR}/source-scan-tmp"
install -d -m 0750 -o root -g chaptera "${CONFIG_DIR}"
install -d -m 0700 -o root -g root "${CREDENTIAL_DIR}"
install -d -m 0755 "${RELEASE}/tools"

install -m 0755 "${ROOT_DIR}/chaptera" "${RELEASE}/chaptera"
install -m 0755 "${ROOT_DIR}/tools/migration_pdf_worker_isolation.py"   "${RELEASE}/tools/migration_pdf_worker_isolation.py"
install -m 0755 "${ROOT_DIR}/run_cloud_reader_host_acceptance.py"   "${RELEASE}/run_cloud_reader_host_acceptance.py"

if [[ ! -f "${CONFIG_DIR}/chaptera.toml" ]]; then
  install -m 0640 -o root -g chaptera     "${ROOT_DIR}/chaptera.reader.prod.example.toml"     "${CONFIG_DIR}/chaptera.toml"
fi

if [[ ! -s "${CREDENTIAL_DIR}/reader_rate_subject_secret" ]]; then
  umask 077
  openssl rand -hex 32 > "${CREDENTIAL_DIR}/reader_rate_subject_secret"
fi
chmod 0600 "${CREDENTIAL_DIR}/reader_rate_subject_secret"

ln -sfn "${RELEASE}" "${CURRENT}"

install -m 0644 "${ROOT_DIR}/chaptera-reader.service"   /etc/systemd/system/chaptera-reader.service
install -m 0644 "${ROOT_DIR}/chaptera-reader.conf.example"   /etc/nginx/sites-available/chaptera-reader
ln -sfn /etc/nginx/sites-available/chaptera-reader   /etc/nginx/sites-enabled/chaptera-reader

runuser -u chaptera -- "${CURRENT}/chaptera"   --config "${CONFIG_DIR}/chaptera.toml" migrate up

systemctl daemon-reload
systemctl enable chaptera-reader.service
nginx -t
systemctl reload nginx
systemctl restart chaptera-reader.service

echo "=== chaptera-reader ==="
systemctl --no-pager --full status chaptera-reader.service || true
echo
echo "=== loopback health ==="
curl -fsS http://127.0.0.1:8080/ >/dev/null
echo "Reader process is serving on 127.0.0.1:8080"
echo
echo "Next: issue TLS with certbot for reader.chaptera.online after DNS points here."
