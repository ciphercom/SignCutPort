const paths: Record<string, string> = {
  import: "M12 3v12m0 0l-4-4m4 4l4-4M4 17v3h16v-3",
  text: "M5 5h14M12 5v14M9 19h6",
  font: "M4 19L9.5 5h1L16 19M6.3 14h7.4M17 12h4M19 10v9",
  undo: "M9 14L4 9l5-5M4 9h10a6 6 0 010 12h-3",
  redo: "M15 14l5-5-5-5M20 9H10a6 6 0 000 12h3",
  duplicate: "M8 8h12v12H8zM4 16V4h12",
  rotate: "M20 12a8 8 0 11-2.3-5.6M20 4v5h-5",
  flipH: "M12 3v18M8 7L3 12l5 5zM16 7l5 5-5 5z",
  flipV: "M3 12h18M7 8l5-5 5 5zM7 16l5 5 5-5z",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  alignLeft: "M4 3v18M8 7h10v4H8zM8 14h6v4H8z",
  alignCenter: "M12 3v18M6 7h12v4H6zM8 14h8v4H8z",
  alignTop: "M3 4h18M7 8h4v10H7zM14 8h4v6h-4z",
  alignBottom: "M3 20h18M7 6h4v10H7zM14 10h4v6h-4z",
  arrange: "M4 4h7v9H4zM13 4h7v5h-7zM13 11h7v9h-7zM4 15h7v5H4z",
  origin: "M4 20h16M4 20V4M4 20l7-7M8 20H4v-4",
  fit: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
  cut: "M6 6a2.5 2.5 0 110 .1M6 18a2.5 2.5 0 110 .1M8 7.5L20 18M8 16.5L20 6",
};

export function Icon({ name, size = 16 }: { name: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d={paths[name] ?? ""} />
    </svg>
  );
}
