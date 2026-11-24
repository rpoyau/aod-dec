// Pure-vector bar chart of the 11-bin Dimonhexon double-slit cross-return delta_3 fixture.
#let fig-double-slit-delta3() = {
  let data = json("/docs/manual/data/double_slit_delta3.json")
  let bins = data.bins
  let chart-w = 13.8cm
  let chart-h = 5.4cm
  let plot-h = 3.8cm
  let max-v = 100
  let col-w = 1.05cm
  let x0 = 1.2cm
  let y-base = 4.3cm

  align(center)[
    #box(width: chart-w, height: chart-h, stroke: 0.4pt + luma(190), inset: 4pt)[
      #place(top + left, line(start: (x0, y-base), end: (x0 + 11 * col-w, y-base), stroke: 0.7pt + black))
      #place(top + left, line(start: (x0, y-base), end: (x0, y-base - plot-h), stroke: 0.7pt + black))
      #for tick in (0, 25, 50, 75, 100) {
        let ty = y-base - (tick / max-v) * plot-h
        place(top + left, line(start: (x0 - 2pt, ty), end: (x0 + 11 * col-w, ty), stroke: 0.25pt + luma(215)))
        place(top + left, dx: 0.1cm, dy: ty - 4pt, text(size: 7.5pt)[#tick])
      }
      #for (i, b) in bins.enumerate() {
        let bx = x0 + i * col-w + 0.12cm
        let h0 = (b.t0 / max-v) * plot-h
        let hx = (b.tx / max-v) * plot-h
        place(top + left, dx: bx, dy: y-base - h0, rect(width: 0.36cm, height: h0, fill: luma(195), stroke: 0.45pt + black))
        place(top + left, dx: bx + 0.39cm, dy: y-base - hx, rect(width: 0.36cm, height: hx, fill: luma(70), stroke: 0.45pt + black))
        place(top + left, dx: bx + 0.14cm, dy: y-base + 3pt, text(size: 7.5pt)[#b.k])
      }
      #place(top + left, dx: x0 + 0.2cm, dy: 0.08cm, text(size: 8pt)[
        #box(width: 7pt, height: 7pt, fill: luma(195), stroke: 0.4pt + black) $T^0_k$ (sum #data.sum_t0) #h(0.8cm)
        #box(width: 7pt, height: 7pt, fill: luma(70), stroke: 0.4pt + black) $T^times_k = T^0_k + C^times_k$ (sum #data.sum_tx, $sum Delta_k = #data.sum_delta$, $sum m_k = #data.sum_mag$)
      ])
    ]
  ]
}
