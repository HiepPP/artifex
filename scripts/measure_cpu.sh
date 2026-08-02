#!/bin/bash
# Average CPU over a window, measured the same way for both builds.
#
#   scripts/measure_cpu.sh <pid> <seconds> [label]
#
# Reports two numbers:
#   ps %cpu      - what `ps -o %cpu=` prints, the number the Swift baseline used
#   cputime      - (delta CPU seconds / wall seconds) * 100, an exact average
set -euo pipefail

PID="$1"
SECONDS_TO_RUN="${2:-60}"
LABEL="${3:-run}"

cpu_seconds() {
    ps -p "$1" -o time= | awk -F: '{ n=NF; s=$n; if (n>1) s += $(n-1)*60; if (n>2) s += $(n-2)*3600; print s }'
}

start_cpu=$(cpu_seconds "$PID")
start_wall=$(date +%s)

samples=()
elapsed=0
while [[ $elapsed -lt $SECONDS_TO_RUN ]]; do
    sleep 2
    samples+=("$(ps -p "$PID" -o %cpu= | tr -d ' ')")
    elapsed=$(( $(date +%s) - start_wall ))
done

end_cpu=$(cpu_seconds "$PID")
end_wall=$(date +%s)

printf '%s\n' "${samples[@]}" | awk -v label="$LABEL" -v dc="$(echo "$end_cpu - $start_cpu" | bc -l)" \
    -v dw="$(( end_wall - start_wall ))" -v rss="$(ps -p "$PID" -o rss= | tr -d ' ')" '
{ sum += $1; if ($1 > max) max = $1; n++ }
END {
    printf "%-22s ps%%cpu avg=%.2f peak=%.2f  cputime avg=%.2f%%  rss=%.0f MB  window=%ds samples=%d\n",
        label, sum/n, max, (dc/dw)*100, rss/1024, dw, n
}'
