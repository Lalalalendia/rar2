#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "run as root" >&2
  exit 1
fi

DEPLOY_USER="${1:-chaptera-deploy}"
DEPLOY_HOME="$(getent passwd "${DEPLOY_USER}" | cut -d: -f6)"
if [[ -z "${DEPLOY_HOME}" || ! -d "${DEPLOY_HOME}" ]]; then
  echo "deploy user/home is unavailable: ${DEPLOY_USER}" >&2
  exit 1
fi

install -d -m 0750 -o "${DEPLOY_USER}" -g "${DEPLOY_USER}" "${DEPLOY_HOME}/chaptera-reader-incoming"

cat >/usr/local/sbin/chaptera-reader-deploy <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--version" ]]; then
  echo "chaptera-reader-deploy-v1"
  exit 0
fi

if [[ $# -ne 2 ]]; then
  echo "usage: chaptera-reader-deploy <commit-sha> <archive>" >&2
  exit 2
fi

SHA="$1"
ARCHIVE="$2"
DEPLOY_USER="chaptera-deploy"
DEPLOY_HOME="$(getent passwd "${DEPLOY_USER}" | cut -d: -f6)"

if [[ ! "${SHA}" =~ ^[0-9a-f]{40}$ ]]; then
  echo "invalid commit sha" >&2
  exit 2
fi

EXPECTED="${DEPLOY_HOME}/chaptera-reader-incoming/${SHA}/chaptera-cloud-reader-linux-amd64.tar.gz"
ARCHIVE_REAL="$(readlink -f "${ARCHIVE}")"
if [[ "${ARCHIVE_REAL}" != "${EXPECTED}" ]]; then
  echo "archive path is outside the admitted deploy slot" >&2
  exit 2
fi
if [[ ! -f "${ARCHIVE_REAL}" ]]; then
  echo "release archive is missing" >&2
  exit 2
fi

WORK="$(mktemp -d /var/tmp/chaptera-reader-deploy.XXXXXX)"
cleanup() { rm -rf "${WORK}"; }
trap cleanup EXIT

tar -xzf "${ARCHIVE_REAL}" -C "${WORK}"
cd "${WORK}"

for required in   chaptera   chaptera.reader.prod.example.toml   chaptera-reader.service   chaptera-reader.conf.example   install-reader-host.sh   tools/migration_pdf_worker_isolation.py   run_cloud_reader_guest_host_acceptance.py   COMMIT_SHA   SHA256SUMS
do
  [[ -e "${required}" ]] || { echo "release packet missing ${required}" >&2; exit 3; }
done

sha256sum -c SHA256SUMS
PACKET_SHA="$(tr -d '\r\n' < COMMIT_SHA)"
[[ "${PACKET_SHA}" == "${SHA}" ]] || {
  echo "packet commit mismatch: ${PACKET_SHA} != ${SHA}" >&2
  exit 3
}

bash ./install-reader-host.sh

if [[ ! -e /etc/letsencrypt/live/reader.chaptera.online/fullchain.pem ]]; then
  certbot --nginx --non-interactive --agree-tos --redirect -d reader.chaptera.online
fi

nginx -t
systemctl is-active --quiet chaptera-reader.service
curl -fsS http://127.0.0.1:8080/live >/dev/null
curl -fsS https://reader.chaptera.online/live >/dev/null
echo "chaptera-reader deploy complete: ${SHA}"
EOF

chmod 0755 /usr/local/sbin/chaptera-reader-deploy
chown root:root /usr/local/sbin/chaptera-reader-deploy

cat >/etc/sudoers.d/chaptera-reader-deploy <<EOF
${DEPLOY_USER} ALL=(root) NOPASSWD: /usr/local/sbin/chaptera-reader-deploy *
EOF
chmod 0440 /etc/sudoers.d/chaptera-reader-deploy
visudo -cf /etc/sudoers.d/chaptera-reader-deploy

echo "Installed restricted Chaptera Reader deploy bridge for ${DEPLOY_USER}."
echo "Test with:"
echo "  sudo -u ${DEPLOY_USER} sudo -n /usr/local/sbin/chaptera-reader-deploy --version"
