#!/usr/bin/env bash

set -euo pipefail

diagram_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
diagram_name="${1:-wateros-architecture}"
source_file="${diagram_dir}/${diagram_name}.mmd"
theme_file="${diagram_dir}/mermaid-theme.mmd"
config_file="${diagram_dir}/mermaid-config.json"
output_file="${diagram_dir}/${diagram_name}.svg"

if [[ ! -f "${source_file}" ]]; then
    echo "diagram source not found: ${source_file}" >&2
    exit 1
fi

combined_file="$(mktemp --suffix=.mmd)"
rendered_file="$(mktemp --suffix=.svg)"
trap 'rm -f "${combined_file}" "${rendered_file}"' EXIT

sed '/^[[:space:]]*%%/d' "${source_file}" > "${combined_file}"
printf '\n' >> "${combined_file}"
sed '/^[[:space:]]*%%/d' "${theme_file}" >> "${combined_file}"

payload="$({
    jq -cn \
        --rawfile code "${combined_file}" \
        --slurpfile mermaid "${config_file}" \
        '{code: $code, mermaid: $mermaid[0], autoSync: true, updateDiagram: true}'
} | node -e '
    const zlib = require("node:zlib");
    const chunks = [];
    process.stdin.on("data", chunk => chunks.push(chunk));
    process.stdin.on("end", () => {
        const input = Buffer.concat(chunks);
        process.stdout.write(zlib.deflateSync(input, { level: 9 }).toString("base64url"));
    });
')"

curl --fail-with-body --silent --show-error \
    --max-time 180 \
    "https://mermaid.ink/svg/pako:${payload}" \
    --output "${rendered_file}"

xmllint --noout "${rendered_file}"
mv "${rendered_file}" "${output_file}"

echo "rendered ${output_file}"
