// Pure-vector schematic of the opposing 3:3:6 chamber sandbox and passive read-only observer wall.
#let fig-sandbox-chamber-schematic() = {
  let summary = json("/docs/manual/data/sandbox_chamber_summary.json")
  align(center)[
    #box(width: 14.4cm, height: 5.0cm, stroke: 0.4pt + luma(180), inset: 6pt)[
      // Chamber box [0..64]^3
      #place(top + left, dx: 0.3cm, dy: 0.5cm, box(width: 8.2cm, height: 3.8cm, stroke: 0.9pt + black, radius: 2pt)[
        #place(top + left, dx: 0.25cm, dy: 0.15cm, text(size: 8.5pt, weight: "bold")[Chamber `box([0..64, 0..64, 0..64])` (walls = open)])
        // Left carrier 3:3:6 +X
        #place(top + left, dx: 0.8cm, dy: 1.55cm, box(width: 2.0cm, height: 0.85cm, stroke: 0.7pt + black, fill: luma(235), radius: 2pt)[
          #align(center + horizon)[#text(size: 8pt)[`3:3:6` $(+X, +1_t)$]]
        ])
        #place(top + left, line(start: (2.85cm, 1.98cm), end: (3.85cm, 1.98cm), stroke: 0.9pt + black))
        // Right carrier 3:3:6 -X
        #place(top + left, dx: 5.4cm, dy: 1.55cm, box(width: 2.0cm, height: 0.85cm, stroke: 0.7pt + black, fill: luma(235), radius: 2pt)[
          #align(center + horizon)[#text(size: 8pt)[`3:3:6` $(-X, -1_t)$]]
        ])
        #place(top + left, line(start: (5.35cm, 1.98cm), end: (4.35cm, 1.98cm), stroke: 0.9pt + black))
        // Collision core zone
        #place(top + left, dx: 3.9cm, dy: 1.68cm, circle(radius: 0.28cm, stroke: 0.8pt + black, fill: luma(160)))
        #place(top + left, dx: 2.0cm, dy: 2.85cm, text(size: 7.5pt)[
          ticks=#summary.ticks, carriers=#summary.carrier_count, shedding=#summary.shedding_event_count, returns=#summary.total_returns, hinges=#summary.total_hinges
        ])
      ])
      // One-way ReadOnly wall
      #place(top + left, line(start: (8.9cm, 0.5cm), end: (8.9cm, 4.3cm), stroke: (paint: black, thickness: 1.0pt, dash: "dashed")))
      #place(top + left, line(start: (8.55cm, 2.4cm), end: (9.55cm, 2.4cm), stroke: 0.8pt + black))
      // Passive observers box
      #place(top + left, dx: 9.6cm, dy: 0.7cm, box(width: 4.3cm, height: 3.4cm, stroke: 0.75pt + black, radius: 3pt, inset: 5pt)[
        #align(center + horizon)[
          #text(size: 8.5pt, weight: "bold")[Read-Only Observers] \
          #v(0.2cm)
          #text(size: 7.8pt)[
            `ReadOnly<ChamberView>` \
            `DeclProbe` + `ProcProbe` \
            #v(0.15cm)
            Zero back-reaction: \
            `hash(0) == hash(1) == hash(2)` \
            (#raw(str(summary.observer_back_reaction_bytes_changed)) bytes drift)
          ]
        ]
      ])
    ]
  ]
}
