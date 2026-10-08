// Math-aware helper fixture, including singleton content.
#let highlighted(body) = context {
  // Determine appropriate highlight colors based on the current text color (light vs dark theme)
  let theme = if text.fill == rgb("#e8edf3") {
    (highlight: rgb("#665c1f")) // Dark mode highlight color (muted gold/green)
  } else {
    (highlight: rgb("#FFFE80")) // Light mode highlight color (bright yellow)
  }

  // Keep adjacent text in one highlight element so its background geometry is
  // calculated consistently instead of once per child (or character).
  // Equations are emitted separately because they need extra vertical room.
  let parts = ()
  let text = []
  let has-text = false

  let children = if body.has("children") { body.children } else { (body,) }
  for child in children {
    if repr(child.func()) == "equation" {
      if has-text {
        parts.push(highlight(text, fill: theme.highlight))
        text = []
        has-text = false
      }

      parts.push(box(
        fill: theme.highlight,
        outset: (y: 0.25em),
      )[$#child.at("body")$])
    } else {
      text = text + child
      has-text = true
    }
  }

  if has-text {
    parts.push(highlight(text, fill: theme.highlight))
  }

  for part in parts {
    part
  }
}

