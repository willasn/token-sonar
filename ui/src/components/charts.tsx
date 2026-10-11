/* TokenSonar · 轻量 SVG 图表（React 版，按容器宽度实时渲染）
 *
 * 由旧 charts.js 平移：平滑折线（Catmull-Rom 转贝塞尔）+ 面积填充 + 悬停提示。
 * 无第三方依赖。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { fmt } from "../lib/format";

let uid = 0;

/** 平滑折线路径（Catmull-Rom 转三次贝塞尔，张力可调） */
function smoothPath(pts: number[][], tension?: number): string {
  if (!pts.length) return "";
  if (pts.length < 3) {
    return "M" + pts.map((p) => p[0] + "," + p[1]).join("L");
  }
  const tn = tension == null ? 0.2 : tension;
  let d = "M" + pts[0][0] + "," + pts[0][1];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] || p2;
    const c1x = p1[0] + (p2[0] - p0[0]) * tn;
    const c1y = p1[1] + (p2[1] - p0[1]) * tn;
    const c2x = p2[0] - (p3[0] - p1[0]) * tn;
    const c2y = p2[1] - (p3[1] - p1[1]) * tn;
    d += "C" + c1x + "," + c1y + " " + c2x + "," + c2y + " " + p2[0] + "," + p2[1];
  }
  return d;
}

function niceMax(v: number): number {
  if (v <= 0) return 1;
  const exp = Math.floor(Math.log10(v));
  const base = Math.pow(10, exp);
  const n = v / base;
  let step = 1;
  if (n > 5) step = 10;
  else if (n > 2) step = 5;
  else if (n > 1) step = 2;
  return step * base;
}

function getVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name);
  return (v && v.trim()) || fallback;
}

/** 容器宽度跟踪 */
function useWidth<T extends HTMLElement>(fallback: number, min: number) {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(fallback);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => setWidth(Math.max(min, el.clientWidth || fallback));
    read();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [fallback, min]);
  return { ref, width };
}

export interface AreaDatum {
  label: string;
  value: number;
}

export function AreaChart({
  data,
  height = 220,
  color,
  format,
  yFormat,
}: {
  data: AreaDatum[];
  height?: number;
  color?: string;
  format?: (v: number) => string;
  yFormat?: (v: number) => string;
}) {
  const { ref, width } = useWidth<HTMLDivElement>(720, 240);
  const svgRef = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState<number | null>(null);

  const padL = 48;
  const padR = 14;
  const padT = 16;
  const padB = 26;

  const values = data.map((d) => Number(d.value) || 0);
  const maxRaw = Math.max(...values.concat([0]));
  const max = niceMax(maxRaw || 1);
  const fmtY = yFormat || ((v: number) => fmt.compact(v));
  const stroke = color || getVar("--accent", "#4d6bfe");

  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const n = data.length;
  const stepX = n > 1 ? innerW / (n - 1) : 0;
  const pts = values.map((v, i) => [
    padL + i * stepX,
    padT + innerH - (max ? (v / max) * innerH : 0),
  ]);
  const line = smoothPath(pts);
  const fillPath = pts.length
    ? line + "L" + pts[pts.length - 1][0] + "," + (padT + innerH) + "L" + pts[0][0] + "," + (padT + innerH) + "Z"
    : "";
  const gid = useMemo(() => "tlgrad" + ++uid, []);

  const rows = 4;
  const grid: React.ReactNode[] = [];
  for (let i = 0; i <= rows; i++) {
    const y = padT + (innerH / rows) * i;
    const val = max - (max / rows) * i;
    grid.push(
      <line
        key={"g" + i}
        x1={padL}
        y1={y}
        x2={width - padR}
        y2={y}
        stroke={getVar("--grid", "rgba(255,255,255,.06)")}
        strokeWidth={1}
      />,
      <text key={"gt" + i} x={padL - 8} y={y + 3.5} textAnchor="end" fontSize={10} fill={getVar("--text-mute", "#63708c")}>
        {fmtY(val)}
      </text>
    );
  }

  const xlabels: React.ReactNode[] = [];
  const maxLabels = Math.min(7, n);
  for (let i = 0; i < maxLabels && n > 0; i++) {
    const idx = Math.round((i * (n - 1)) / (maxLabels - 1 || 1));
    const x = padL + idx * stepX;
    xlabels.push(
      <text
        key={"x" + i}
        x={x}
        y={height - 6}
        textAnchor="middle"
        fontSize={10}
        fill={getVar("--text-mute", "#63708c")}
      >
        {fmt.shortDate(data[idx].label)}
      </text>
    );
  }

  const dots: React.ReactNode[] = [];
  if (n <= 32) {
    pts.forEach((p, i) => {
      if (values[i] > 0) {
        dots.push(<circle key={"d" + i} cx={p[0]} cy={p[1]} r={2} fill={stroke} opacity={0.55} />);
      }
    });
  }

  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const el = svgRef.current;
    if (!el || n === 0) return;
    const rect = el.getBoundingClientRect();
    const scale = width / rect.width;
    const x = (e.clientX - rect.left) * scale;
    let idx = Math.round((x - padL) / (stepX || 1));
    idx = Math.max(0, Math.min(n - 1, idx));
    setHover(idx);
  };

  const hoverP = hover != null ? pts[hover] : null;
  let tip: { x: number; y: number; w: number; text: string } | null = null;
  if (hover != null && hoverP) {
    const text =
      fmt.shortDate(data[hover].label) + " · " + (format ? format(values[hover]) : fmt.compact(values[hover]));
    const w = text.length * 6.2 + 16;
    let tx = hoverP[0] + 10;
    if (tx + w > width - padR) tx = hoverP[0] - w - 10;
    const ty = Math.max(padT, hoverP[1] - 26);
    tip = { x: tx, y: ty, w, text };
  }

  return (
    <div ref={ref} style={{ width: "100%" }}>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        height={height}
        className="tl-chart"
        onMouseMove={onMove}
        onMouseLeave={() => setHover(null)}
      >
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={stroke} stopOpacity={0.38} />
            <stop offset="100%" stopColor={stroke} stopOpacity={0} />
          </linearGradient>
        </defs>
        {grid}
        <path d={fillPath} fill={`url(#${gid})`} />
        <path d={line} fill="none" stroke={stroke} strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
        {dots}
        {xlabels}
        {hoverP && tip && (
          <g className="tl-hover">
            <line
              x1={hoverP[0]}
              x2={hoverP[0]}
              y1={padT}
              y2={padT + innerH}
              stroke={stroke}
              strokeWidth={1}
              strokeDasharray="3 3"
              opacity={0.8}
            />
            <circle cx={hoverP[0]} cy={hoverP[1]} r={4} fill={getVar("--panel", "#111826")} stroke={stroke} strokeWidth={2} />
            <g className="tl-tip" transform={`translate(${tip.x},${tip.y})`}>
              <rect rx={6} height={22} width={tip.w} fill={getVar("--panel-3", "#1c2739")} stroke={getVar("--border-strong", "rgba(255,255,255,.13)")} />
              <text fontSize={10.5} fill={getVar("--text", "#e9eefb")} dy={14.5} dx={8}>
                {tip.text}
              </text>
            </g>
          </g>
        )}
      </svg>
    </div>
  );
}

/** 迷你趋势图 */
export function Sparkline({
  values,
  height = 34,
  color,
}: {
  values: number[];
  height?: number;
  color?: string;
}) {
  const { ref, width } = useWidth<HTMLDivElement>(180, 60);
  const stroke = color || getVar("--accent", "#4d6bfe");
  const gid = useMemo(() => "tlspark" + ++uid, []);
  const vals = (values || []).map((v) => Number(v) || 0);

  if (!vals.length) {
    return (
      <div ref={ref} style={{ width: "100%" }}>
        <svg viewBox={`0 0 ${width} ${height}`} width="100%" height={height}>
          <line
            x1={0}
            y1={height / 2}
            x2={width}
            y2={height / 2}
            stroke={getVar("--grid", "rgba(255,255,255,.06)")}
            strokeDasharray="3 3"
          />
        </svg>
      </div>
    );
  }

  const max = Math.max(...vals.concat([0])) || 1;
  const stepX = vals.length > 1 ? width / (vals.length - 1) : 0;
  const pts = vals.map((v, i) => [i * stepX, height - 3 - (v / max) * (height - 8)]);
  const line = smoothPath(pts, 0.16);
  const fill = line + "L" + width + "," + height + "L0," + height + "Z";

  return (
    <div ref={ref} style={{ width: "100%" }}>
      <svg viewBox={`0 0 ${width} ${height}`} width="100%" height={height}>
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={stroke} stopOpacity={0.35} />
            <stop offset="100%" stopColor={stroke} stopOpacity={0} />
          </linearGradient>
        </defs>
        <path d={fill} fill={`url(#${gid})`} />
        <path d={line} fill="none" stroke={stroke} strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </div>
  );
}
