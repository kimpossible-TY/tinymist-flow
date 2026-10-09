// Imported reference output has the helper's span, rather than its call site.
#let local-tag-scope(body) = body((ref: name => ref(label(name))))
