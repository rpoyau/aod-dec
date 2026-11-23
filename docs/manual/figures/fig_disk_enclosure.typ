// Pure-vector convergence plot of cubical unit-disk enclosures A_n^- <= A_disk <= A_n^+ vs C_n = 8.
#let fig-disk-enclosure() = {
  let data = json("/docs/manual/data/disk_enclosure.json")
  let rows = data.resolutions
  let w = 13.6cm
  let h = 5.2cm
  let x0 = 1.3cm
  let y-base = 4.2cm
  let plot-h = 3.5cm
  let max-v = 9
  let step-x = 2.3cm

  align(center)[
    #box(width: w, height: h, stroke: 0.4pt + luma(190), inset: 4pt)[
      #place(top + left, line(start: (x0, y-base), end: (x0 + 5 * step-x, y-base), stroke: 0.7pt + black))
      #place(top + left, line(start: (x0, y-base), end: (x0, y-base - plot-h), stroke: 0.7pt + black))
      #for tick in (0, 2, 4, 6, 8) {
        let ty = y-base - (tick / max-v) * plot-h
        place(top + left, line(start: (x0 - 2pt, ty), end: (x0 + 5 * step-x, ty), stroke: 0.25pt + luma(215)))
        place(top + left, dx: 0.2cm, dy: ty - 4pt, text(size: 7.5pt)[#tick])
      }
      // Constant axial perimeter C_n = 8 reference line
      #let y-c8 = y-base - (8 / max-v) * plot-h
      #place(top + left, line(start: (x0, y-c8), end: (x0 + 5 * step-x, y-c8), stroke: (paint: black, thickness: 0.8pt, dash: "dashed")))
      #for (i, r) in rows.enumerate() {
        let cx = x0 + (i + 0.5) * step-x
        let y-lo = y-base - ((r.a_minus_num / r.a_minus_den) / max-v) * plot-h
        let y-hi = y-base - ((r.a_plus_num / r.a_plus_den) / max-v) * plot-h
        let bar-h = calc.max(y-lo - y-hi, 1.5pt)
        place(top + left, dx: cx - 0.35cm, dy: y-hi, rect(width: 0.7cm, height: bar-h, fill: luma(175), stroke: 0.6pt + black))
        place(top + left, dx: cx - 0.5cm, dy: y-base + 3pt, text(size: 7.5pt)[$n = #r.n$])
        place(top + left, dx: cx - 0.7cm, dy: y-hi - 11pt, text(size: 6.8pt)[\[#r.a_minus_num/#r.a_minus_den, #r.a_plus_num/#r.a_plus_den\]])
      }
      #place(top + left, dx: x0 + 0.2cm, dy: 0.08cm, text(size: 8pt)[
        Dashed line: axial perimeter $C_n = 8$ (invariant across all $n$); shaded intervals: exact $[A_n^-, A_n^+]$
      ])
    ]
  ]
}
