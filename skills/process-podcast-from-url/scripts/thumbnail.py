#!/usr/bin/env python3
"""Episode thumbnail checker / extractor — skips leading black frames.

Only needs ffmpeg + ffprobe on PATH (the skill already requires them).
Prints a single JSON object on stdout; exits 0 even for a "black" verdict
(the caller decides), exits 2 on usage/environment errors.

  check   <image-file-or-url>         -> {"black":bool,"meanLuma":x,"spread":y}
  extract <video-file> [-o out.jpg]   -> {"path":...,"at":seconds,"meanLuma":x}

Black verdict: mean luma < 16/255 and (max-min) spread < 32/255 — a real
picture almost never sits that dark and that flat at once.
"""
import json, os, subprocess, sys, tempfile, urllib.request

BLACK_MEAN, BLACK_SPREAD = 16.0, 32.0


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def luma_of_image(path):
    """Mean YAVG and (YMAX-YMIN) spread of an image, 0-255 scale."""
    proc = run(["ffmpeg", "-hide_banner", "-i", path, "-vf",
                "scale=64:64,signalstats,metadata=print:key=lavfi.signalstats.YAVG,metadata=print:key=lavfi.signalstats.YMAX,metadata=print:key=lavfi.signalstats.YMIN",
                "-f", "null", "-"])
    yavg = ymax = ymin = None
    for line in proc.stderr.splitlines():
        if "YAVG" in line:
            yavg = float(line.split("=")[-1])
        elif "YMAX" in line:
            ymax = float(line.split("=")[-1])
        elif "YMIN" in line:
            ymin = float(line.split("=")[-1])
    if yavg is None:
        raise RuntimeError("ffmpeg signalstats produced no YAVG (bad image?)")
    return yavg, (ymax - ymin) if (ymax is not None and ymin is not None) else 255.0


def fetch_to_temp(source):
    if not source.startswith(("http://", "https://")):
        return source
    fd, tmp = tempfile.mkstemp(suffix=os.path.splitext(source.split("?")[0])[1] or ".img")
    os.close(fd)
    req = urllib.request.Request(source, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=30) as resp, open(tmp, "wb") as out:
        out.write(resp.read(20 * 1024 * 1024))
    return tmp


def video_duration(video):
    proc = run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                "-of", "csv=p=0", video])
    return float(proc.stdout.strip())


def first_visible_time(video):
    """First second that is NOT part of a leading black run (ffmpeg blackdetect)."""
    proc = run(["ffmpeg", "-hide_banner", "-i", video, "-vf",
                "blackdetect=d=0.3:pix_th=0.10", "-an", "-f", "null", "-"])
    start_at = 0.0
    for line in proc.stderr.splitlines():
        if "black_start" not in line:
            continue
        fields = {}
        for pair in line.split():
            if ":" in pair:
                key, _, value = pair.partition(":")
                fields[key] = value
        try:
            bstart, bend = float(fields["black_start"]), float(fields["black_end"])
        except (KeyError, ValueError):
            continue
        if bstart <= 0.5:  # a black run at the head: skip past it
            start_at = max(start_at, bend)
    return start_at


def extract(video, out):
    duration = video_duration(video)
    candidates = [max(0.0, first_visible_time(video) + 0.5)]
    candidates += [duration * f for f in (0.05, 0.1, 0.15, 0.25, 0.4, 0.6)]
    for at in candidates:
        if at >= duration:
            continue
        proc = run(["ffmpeg", "-y", "-hide_banner", "-ss", f"{at:.2f}", "-i", video,
                    "-frames:v", "1", "-vf", "scale='min(1280,iw)':-2", "-q:v", "3", out])
        if proc.returncode or not os.path.isfile(out) or not os.path.getsize(out):
            continue
        yavg, spread = luma_of_image(out)
        if yavg >= BLACK_MEAN or spread >= BLACK_SPREAD:
            return {"path": out, "at": round(at, 2), "meanLuma": round(yavg, 1)}
    raise RuntimeError("no non-black frame found in video")


def main():
    argv = sys.argv[1:]
    if len(argv) >= 2 and argv[0] == "check":
        src = fetch_to_temp(argv[1])
        yavg, spread = luma_of_image(src)
        print(json.dumps({"black": yavg < BLACK_MEAN and spread < BLACK_SPREAD,
                          "meanLuma": round(yavg, 1), "spread": round(spread, 1)}))
        return
    if len(argv) >= 2 and argv[0] == "extract":
        out = argv[argv.index("-o") + 1] if "-o" in argv else "thumbnail.jpg"
        print(json.dumps(extract(argv[1], out)))
        return
    print(__doc__)
    sys.exit(2)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps({"error": str(error)[:300]}))
        sys.exit(1)
