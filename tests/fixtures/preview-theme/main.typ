#let dark = sys.inputs.at("theme", default: "light") == "dark"
#set page(width: 240pt, height: 300pt, margin: 24pt, fill: if dark { rgb("#17212f") } else { white })
#set text(size: 12pt, fill: if dark { rgb("#edf2f7") } else { rgb("#17212f") })
#if dark {
  [Native dark palette]
} else {
  [Native light palette]
}

Theme following uses the document's own colors, not inversion.

#for number in range(2, 21) {
  pagebreak()
  [= Reading page #number]
  [This passage can be selected while the device changes appearance.]
}
