interface RestartSparklineProps {
  data: { time: number; count: number }[];
  width?: number;
  height?: number;
}

export function RestartSparkline({ data, width = 60, height = 16 }: RestartSparklineProps) {
  const latestCount = data.length > 0 ? data[data.length - 1].count : 0;
  const allZeros = data.every((d) => d.count === 0);

  // If no meaningful data, just show the count
  if (data.length < 2 || allZeros) {
    return (
      <span
        className="inline-flex items-center font-mono text-xs tabular-nums"
        style={{ color: latestCount === 0 ? "#64748b" : "#f59e0b" }}
      >
        {latestCount}
      </span>
    );
  }

  // Determine line color based on trend
  const prevCount = data.length >= 2 ? data[data.length - 2].count : 0;
  let lineColor: string;
  if (latestCount === 0) {
    lineColor = "#64748b"; // slate
  } else if (latestCount > prevCount) {
    lineColor = "#ef4444"; // red - restarts increasing
  } else {
    lineColor = "#f59e0b"; // amber - stable but non-zero
  }

  // Normalize data points to fit within SVG dimensions
  const padding = 1;
  const drawWidth = width - padding * 2;
  const drawHeight = height - padding * 2;

  const minCount = Math.min(...data.map((d) => d.count));
  const maxCount = Math.max(...data.map((d) => d.count));
  const countRange = maxCount - minCount || 1; // avoid division by zero

  const points = data.map((d, i) => {
    const x = padding + (i / (data.length - 1)) * drawWidth;
    // Invert Y so higher values go up
    const y = padding + drawHeight - ((d.count - minCount) / countRange) * drawHeight;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });

  return (
    <span className="inline-flex items-center gap-1.5">
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        className="inline-block align-middle"
        style={{ minWidth: width }}
      >
        <polyline
          points={points.join(" ")}
          fill="none"
          stroke={lineColor}
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <span
        className="font-mono text-xs tabular-nums"
        style={{ color: lineColor, lineHeight: 1 }}
      >
        {latestCount}
      </span>
    </span>
  );
}
