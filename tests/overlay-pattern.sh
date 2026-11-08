#!/bin/sh

set -eu

usage() {
    cat <<'EOF'
Usage: tests/overlay-pattern.sh [--help]

Generate a local moving video test pattern and timed external subtitles, then
play them fullscreen in MPV. Files are temporary and removed when MPV exits.
EOF
}

case $# in
    0) ;;
    1)
        case $1 in
            --help|-h) usage; exit 0 ;;
            *) printf 'Unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
        esac
        ;;
    *) usage >&2; exit 2 ;;
esac

for tool in ffmpeg mpv mktemp; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Required command not found: %s\n' "$tool" >&2
        exit 127
    fi
done

printf '%s\n' 'Requesting MPV VAAPI hardware decoding with the VAAPI GPU context; failure to initialize VAAPI fails this pattern run. KWin/GPU may still choose composition instead of overlay promotion. Check the KMS diagnostic: it is authoritative for the reported plane layout.'

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/overlay-pattern.XXXXXX") || exit 1
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

video=$tmpdir/overlay-pattern.mp4
subs=$tmpdir/overlay-pattern.srt

# Synthetic lavfi sources only: this does not capture the desktop.
ffmpeg -hide_banner -loglevel error -y \
    -f lavfi \
    -i 'testsrc2=size=1920x1080:rate=30:duration=15' \
    -vf "drawbox=x='mod(t*180,1920)':y=180:w=220:h=120:color=black:t=fill,drawbox=x='mod(t*180+960,1920)':y=780:w=220:h=120:color=white:t=fill" \
    -an -c:v libx264 -preset ultrafast -pix_fmt yuv420p "$video"

cat >"$subs" <<'EOF'
1
00:00:01,000 --> 00:00:03,500
WHITE TEXT / BLACK OUTLINE

2
00:00:04,000 --> 00:00:06,500
BLACK TEXT / WHITE BACKPLATE

3
00:00:07,000 --> 00:00:09,500
TOP-CENTER OVERLAY TEST

4
00:00:10,000 --> 00:00:14,500
BOTTOM-CENTER OVERLAY TEST
EOF

mpv --no-config --fullscreen --keep-open=no --hwdec=vaapi --hwdec-codecs=all \
    --vo=gpu --gpu-api=opengl \
    --sub-file="$subs" --sid=1 \
    --sub-color='#FFFFFF' --sub-border-color='#000000' --sub-border-size=3 \
    --osd-level=0 "$video"
