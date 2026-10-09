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
