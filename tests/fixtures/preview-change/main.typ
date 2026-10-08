#let dark = sys.inputs.at("theme", default: "light") == "dark"
#set page(width: 240pt, height: 300pt, margin: 24pt, fill: if dark { rgb("#17212f") } else { white })
#set text(size: 12pt, fill: if dark { rgb("#edf2f7") } else { rgb("#17212f") })
= Last-edit restoration

#for number in range(2, 131) {
  pagebreak()
  [= Reading page #number]
  if number == 120 { include "passage.typ" } else { [An unchanged passage.] }
}
