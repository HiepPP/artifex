#!/bin/bash
# Per-second CPU windows for a pid. Prints each window and the peak.
# 100% == one core.
set -euo pipefail
PID="$1"; N="${2:-12}"; LABEL="${3:-scroll}"
cpu() { ps -p "$1" -o time= | awk -F: '{ n=NF; s=$n; if (n>1) s+=$(n-1)*60; if (n>2) s+=$(n-2)*3600; print s }'; }
prev=$(cpu "$PID"); peak=0; out=""
for _ in $(seq 1 "$N"); do
  sleep 1
  cur=$(cpu "$PID")
  d=$(echo "($cur - $prev) * 100" | bc -l)
  prev=$cur
  out+=$(printf "%.0f " "$d")
  peak=$(echo "if ($d > $peak) $d else $peak" | bc -l)
done
printf "%-26s windows(%%core): %s\n" "$LABEL" "$out"
printf "%-26s peak: %.0f%% of one core\n" "$LABEL" "$peak"
