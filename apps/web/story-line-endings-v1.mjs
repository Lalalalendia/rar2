// Exact text separator bridge for the source-neutral browser textarea.
// Browser textarea.value normalizes CR and CRLF to LF; this helper retains the
// original Story's unambiguous line break convention rather than silently
// mutating unrelated paragraphs. It never grants Native PUB Save authority.
export function prepareStoryTextForTextarea(original) {
  if (typeof original !== "string") throw new TypeError("Story text must be a string");
  const terminal_cr = original.endsWith("\r");
  const body = terminal_cr ? original.slice(0, -1) : original;
  const crlf = body.includes("\r\n");
  const remainder = body.replace(/\r\n/g, "");
  const bareCr = remainder.includes("\r");
  const bareLf = remainder.includes("\n");
  if (Number(crlf) + Number(bareCr) + Number(bareLf) > 1) {
    throw new Error("mixed Publisher Story line endings cannot be edited in the bounded textarea");
  }
  const line_ending_kind = crlf ? "crlf" : bareCr ? "cr" : bareLf ? "lf" : "none";
  return {
    value: body.replace(/\r\n|\r/g, "\n"),
    terminal_cr,
    line_ending_kind,
  };
}

export function restoreStoryTextFromTextarea(value, sourceProfile) {
  if (typeof value !== "string") throw new TypeError("textarea text must be a string");
  if (value.includes("\r")) {
    throw new Error("textarea must supply canonical LF-only input");
  }
  const kind = sourceProfile?.line_ending_kind;
  if (!["none", "cr", "crlf", "lf"].includes(kind) ||
      typeof sourceProfile?.terminal_cr !== "boolean") {
    throw new TypeError("source Story separator profile is required");
  }
  if (kind === "none" && value.includes("\n")) {
    throw new Error("new paragraph separator is not grounded for this Story");
  }
  const newline = kind === "crlf" ? "\r\n" : kind === "cr" ? "\r" : "\n";
  return value.replace(/\n/g, newline) + (sourceProfile.terminal_cr ? "\r" : "");
}


/**
 * Convert exact, unmodified textarea UTF-16 selection offsets to canonical
 * Publisher Story Unicode-scalar offsets. CRLF is one textarea newline but TWO
 * source scalars; terminal source CR is intentionally absent from textarea.
 * Never infer source content or authorize an edit from an altered textarea.
 */
export function mapUnchangedTextareaSelectionToSourceScalarRange(
  originalStory, sourceProfile, textareaValue, startUtf16, endUtf16,
) {
  const projected = prepareStoryTextForTextarea(originalStory);
  if (!sourceProfile || projected.terminal_cr !== sourceProfile.terminal_cr ||
      projected.line_ending_kind !== sourceProfile.line_ending_kind ||
      projected.value !== textareaValue ||
      restoreStoryTextFromTextarea(textareaValue, sourceProfile) !== originalStory) {
    throw new Error("font range needs the unchanged exact Publisher Story and separator profile");
  }
  if (!Number.isInteger(startUtf16) || !Number.isInteger(endUtf16) ||
      startUtf16 < 0 || endUtf16 > textareaValue.length || startUtf16 >= endUtf16) {
    throw new Error("choose a nonempty exact text range");
  }
  for (const offset of [startUtf16, endUtf16]) {
    const left = textareaValue.charCodeAt(offset - 1);
    const right = textareaValue.charCodeAt(offset);
    if (offset > 0 && offset < textareaValue.length &&
        left >= 0xd800 && left <= 0xdbff &&
        right >= 0xdc00 && right <= 0xdfff) {
      throw new Error("font range cannot split a Unicode surrogate pair");
    }
  }
  const toScalar = offset => {
    const prefix = textareaValue.slice(0, offset);
    const restoredPrefix = sourceProfile.line_ending_kind === "crlf"
      ? prefix.replace(/\n/g, "\r\n")
      : sourceProfile.line_ending_kind === "cr"
        ? prefix.replace(/\n/g, "\r")
        : prefix;
    return Array.from(restoredPrefix).length;
  };
  return {
    start_scalar: toScalar(startUtf16),
    end_scalar: toScalar(endUtf16),
  };
}
