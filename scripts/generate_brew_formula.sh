#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  generate_brew_formula.sh \
    --version <version> \
    --repo <owner/repo> \
    --darwin-amd64-sha <sha256> \
    --darwin-arm64-sha <sha256> \
    --output <path>
EOF
}

version=""
repo=""
darwin_amd64_sha=""
darwin_arm64_sha=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      version="$2"
      shift 2
      ;;
    --repo)
      repo="$2"
      shift 2
      ;;
    --darwin-amd64-sha)
      darwin_amd64_sha="$2"
      shift 2
      ;;
    --darwin-arm64-sha)
      darwin_arm64_sha="$2"
      shift 2
      ;;
    --output)
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${version}" || -z "${repo}" || -z "${darwin_amd64_sha}" || -z "${darwin_arm64_sha}" || -z "${output}" ]]; then
  usage >&2
  exit 1
fi

cat > "${output}" <<EOF
class Toolkit < Formula
  desc "Safety kit between AI coding agents and sensitive services"
  homepage "https://github.com/${repo}"
  license "MIT"
  version "${version}"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/${repo}/releases/download/v${version}/toolkit-${version}-darwin-arm64.tar.gz"
      sha256 "${darwin_arm64_sha}"
    else
      url "https://github.com/${repo}/releases/download/v${version}/toolkit-${version}-darwin-amd64.tar.gz"
      sha256 "${darwin_amd64_sha}"
    end
  end

  def install
    bin.install "toolkit"
    bin.install "tkpsql"
    bin.install "tkmsql"
    bin.install "tkdbr"
    bin.install "toolkit-daemon"
    libexec.install "libexec/setup-daemon.sh"
  end

  def post_install
    opoo "Run the following to complete daemon setup:"
    opoo "  sudo #{opt_libexec}/setup-daemon.sh"
  end

  def caveats
    <<~EOS
      Complete daemon setup by running (requires sudo):

        sudo #{opt_libexec}/setup-daemon.sh

      This is required after both fresh installs and upgrades.

      Then add your connections:

        toolkit config edit

      Verify the daemon is running:

        toolkit status

      For Databricks OAuth login, run as _toolkit after daemon setup:

        sudo -u _toolkit env HOME=/var/lib/toolkit tkdbr --conn <name> auth login
    EOS
  end

  test do
    assert_match "Usage", shell_output("#{bin}/toolkit --help")
  end
end
EOF
