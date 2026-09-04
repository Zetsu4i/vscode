import { useEffect, useMemo, useRef, useState } from "react";
import { ipc } from "../../ipc";
import { formatSize } from "../../util/paths";

const BYTES_PER_ROW = 16;
const ROW_HEIGHT = 18;
const OVERSCAN = 16;
const MAX_BYTES = 256 * 1024;

function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function asciiByte(b: number): string {
  return b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
}

/**
 * Virtualized hex viewer: offset | hex bytes (2×8 groups) | ASCII column.
 * Loads up to 256 KB via the Rust byte-read path; only visible rows render.
 */
export default function HexView({ path }: { path: string }) {
  const [bytes, setBytes] = useState<Uint8Array | null>(null);
  const [size, setSize] = useState(0);
  const [truncated, setTruncated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(600);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setBytes(null);
    setError(null);
    ipc
      .readFileBytes(path, MAX_BYTES)
      .then((res) => {
        if (cancelled) return;
        setBytes(decodeBase64(res.dataB64));
        setSize(res.size);
        setTruncated(res.truncated);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [path]);

  const rows = useMemo(() => {
    if (!bytes) return [];
    const out: { offset: number; hex: string[]; ascii: string }[] = [];
    for (let off = 0; off < bytes.length; off += BYTES_PER_ROW) {
      const slice = bytes.subarray(off, off + BYTES_PER_ROW);
      const hex: string[] = [];
      for (let i = 0; i < BYTES_PER_ROW; i++) {
        hex.push(i < slice.length ? slice[i].toString(16).padStart(2, "0") : "  ");
      }
      let ascii = "";
      for (let i = 0; i < slice.length; i++) ascii += asciiByte(slice[i]);
      out.push({ offset: off, hex, ascii });
    }
    return out;
  }, [bytes]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    setViewportH(el.clientHeight);
  };

  const total = rows.length;
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(total, Math.ceil((scrollTop + viewportH) / ROW_HEIGHT) + OVERSCAN);
  const visible = rows.slice(first, last);

  if (error) {
    return <div className="hex-view hex-error">Failed to read bytes: {error}</div>;
  }

  if (!bytes) {
    return <div className="hex-view hex-loading">Loading bytes…</div>;
  }

  return (
    <div className="hex-view">
      <div className="hex-header">
        <i className="codicon codicon-file-binary" />
        <span className="hex-title">{path.split(/[\\/]/).pop()}</span>
        <span className="hex-size">{formatSize(size)}</span>
        {truncated && (
          <span className="hex-truncated">
            showing first {formatSize(bytes.length)} of {formatSize(size)}
          </span>
        )}
      </div>
      <div className="hex-columns" aria-hidden>
        <span className="hex-offset">Offset</span>
        <span className="hex-hex">
          00 01 02 03 04 05 06 07&nbsp;&nbsp;08 09 0a 0b 0c 0d 0e 0f
        </span>
        <span className="hex-ascii">ASCII</span>
      </div>
      <div className="hex-scroll" ref={scrollRef} onScroll={onScroll}>
        <div style={{ height: total * ROW_HEIGHT, position: "relative" }}>
          <div
            style={{
              position: "absolute",
              top: first * ROW_HEIGHT,
              left: 0,
              right: 0,
            }}
          >
            {visible.map((row) => (
              <div
                key={row.offset}
                className="hex-row"
                style={{ height: ROW_HEIGHT }}
              >
                <span className="hex-offset">
                  {row.offset.toString(16).padStart(8, "0")}
                </span>
                <span className="hex-hex">
                  <span>{row.hex.slice(0, 8).join(" ")}</span>
                  <span className="hex-gap">{row.hex.slice(8).join(" ")}</span>
                </span>
                <span className="hex-ascii">{row.ascii}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
