#!/bin/sh
# Prints the Homebrew formula for a release tag, with checksums from the
# release's SHA256SUMS file.
set -eu

tag=$1
sums=$2

sum() {
	awk -v f="pajama-$tag-$1.tar.gz" '$2 == f { print $1 }' "$sums" | grep -E '^[0-9a-f]{64}$'
}

# Assignments stop the script when an archive is missing from the sums.
darwin_arm64=$(sum darwin-arm64)
darwin_amd64=$(sum darwin-amd64)
linux_arm64=$(sum linux-arm64)
linux_amd64=$(sum linux-amd64)

sed \
	-e "s/@TAG@/$tag/g" \
	-e "s/@DARWIN_ARM64@/$darwin_arm64/" \
	-e "s/@DARWIN_AMD64@/$darwin_amd64/" \
	-e "s/@LINUX_ARM64@/$linux_arm64/" \
	-e "s/@LINUX_AMD64@/$linux_amd64/" \
	"$(dirname "$0")/pajama.rb"
