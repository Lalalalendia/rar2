// This is a source-safe projection of chaptera.reader-compatibility-report.v1.
// The Cloud Reader owns all classification, limitations and route decisions.
// Only fields admitted here may cross into public check records / email.
export type CanonicalState =
  | 'opens_normally'
  | 'needs_review'
  | 'opens_with_salvage'
  | 'unsupported';

export type CanonicalOutputRoute =
  | 'available'
  | 'available_with_limitations'
  | 'unavailable'
  | 'not_applicable'
  | 'not_verified';

export type CanonicalReport = {
  protocolVersion: 'chaptera.reader-compatibility-report.v1';
  sourceSha256: string;
  state: CanonicalState;
  contentSummary?: Partial<Record<
    'page_count' | 'text_frame_count' | 'picture_frame_count' |
    'table_count' | 'other_node_count' | 'story_count' |
    'image_resource_count' | 'recovered_text_range_count' |
    'recovered_image_count' | 'recovered_geometry_count', number>>;
  limitations: { code: string; message: string }[];
  outputRoutes: {
    readOnlyPreview: CanonicalOutputRoute;
    salvageRecovery: CanonicalOutputRoute;
    editableIdml: CanonicalOutputRoute;
    editableOdg: CanonicalOutputRoute;
  };
  recommendedNextStep:
    | 'migration_pilot_preview'
    | 'review_preview_before_migration'
    | 'rescue_review'
    | 'unsupported_or_manual_review';
};

const classificationByState: Record<CanonicalState, string> = {
  opens_normally: 'supported',
  needs_review: 'partial',
  opens_with_salvage: 'salvage',
  unsupported: 'unsupported',
};
const nextByState: Record<CanonicalState, CanonicalReport['recommendedNextStep']> = {
  opens_normally: 'migration_pilot_preview',
  needs_review: 'review_preview_before_migration',
  opens_with_salvage: 'rescue_review',
  unsupported: 'unsupported_or_manual_review',
};

const safeMessages: Record<string, string> = {
  overlap_order_may_differ: 'Overlapping objects may appear in a different order than in Publisher.',
  object_identification_partial: 'Some document objects are only partially identified in this preview.',
  image_preview_unavailable: 'Some embedded images are unavailable in this preview.',
  text_layout_may_differ: 'Some text layout may differ from Microsoft Publisher.',
  source_font_family_unresolved: 'Some source text has no stable font-family identity, so exact source typography cannot be selected.',
  font_resource_unavailable: 'A source font family is known, but its exact configured font resource is unavailable.',
  font_substitution: 'Some text uses a substitute font and may wrap or size differently.',
  preview_fidelity_warning: 'The preview contains known display limitations.',
  preview_limitation_other: 'The preview contains an additional limitation that should be reviewed before migration.',
  preview_limitations: 'The preview has known limitations and should be reviewed before migration.',
  recovered_text_incomplete: 'Some text could not be recovered from the source file.',
  recovered_text_needs_review: 'Recovered text exists, but some text relationships remain ambiguous.',
  recovered_images_incomplete: 'Some image facts could not be recovered from the source file.',
  recovered_geometry_incomplete: 'Page placement and geometry could not be fully recovered.',
  recovery_limitation_other: 'Recovery contains an additional limitation that requires review.',
  recovery_mode: 'Only source-backed recovered facts are available; normal page layout is not claimed.',
  automatic_open_unavailable: 'Chaptera cannot currently produce a trustworthy preview for this file.',
};
const countKeys = [
  'page_count', 'text_frame_count', 'picture_frame_count', 'table_count',
  'other_node_count', 'story_count', 'image_resource_count',
  'recovered_text_range_count', 'recovered_image_count', 'recovered_geometry_count',
] as const;

function object(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function canonicalState(value: unknown): value is CanonicalState {
  return value === 'opens_normally' || value === 'needs_review'
    || value === 'opens_with_salvage' || value === 'unsupported';
}

export function projectCanonicalReport(value: unknown, exactSourceSha: string): CanonicalReport | null {
  const source = object(value);
  if (!source || source.protocol_version !== 'chaptera.reader-compatibility-report.v1'
    || typeof source.source_sha256 !== 'string'
    || !/^[0-9a-f]{64}$/.test(source.source_sha256)
    || source.source_sha256 !== exactSourceSha
    || !canonicalState(source.state)
    || source.engine_classification !== classificationByState[source.state]
    || source.recommended_next_step !== nextByState[source.state]) {
    return null;
  }

  const routes = object(source.output_routes);
  if (!routes
    || !['available', 'available_with_limitations', 'unavailable'].includes(String(routes.read_only_preview))
    || !['not_applicable', 'available', 'unavailable'].includes(String(routes.salvage_recovery))
    // The currently deployed canonical report admits no exact-source editable route.
    // Do not convert generic output availability into an unsupported editable claim.
    || routes.editable_idml !== 'not_verified' || routes.editable_odg !== 'not_verified') {
    return null;
  }

  if ((source.state === 'opens_normally' && (routes.read_only_preview !== 'available' || routes.salvage_recovery !== 'not_applicable'))
    || (source.state === 'needs_review' && (routes.read_only_preview !== 'available_with_limitations' || routes.salvage_recovery !== 'not_applicable'))
    || (source.state === 'opens_with_salvage' && (routes.read_only_preview !== 'unavailable' || routes.salvage_recovery !== 'available'))
    || (source.state === 'unsupported' && (routes.read_only_preview !== 'unavailable' || routes.salvage_recovery !== 'unavailable'))) {
    return null;
  }

  const incomingLimitations = source.limitations;
  if (!Array.isArray(incomingLimitations) || incomingLimitations.length > 16) return null;
  const limitations: CanonicalReport['limitations'] = [];
  const seen = new Set<string>();
  for (const entry of incomingLimitations) {
    const item = object(entry);
    if (!item || typeof item.code !== 'string' || typeof item.message !== 'string'
      || safeMessages[item.code] !== item.message || seen.has(item.code)) return null;
    seen.add(item.code);
    limitations.push({ code: item.code, message: safeMessages[item.code] });
  }

  const summary = source.content_summary;
  if (source.state !== 'unsupported' && !object(summary)) return null;
  if (source.state === 'unsupported' && summary != null) return null;
  const safeSummary: CanonicalReport['contentSummary'] = {};
  if (summary != null) {
    const from = object(summary);
    if (!from) return null;
    for (const key of countKeys) {
      const count = from[key];
      if (count === undefined || count === null) continue;
      if (typeof count !== 'number' || !Number.isSafeInteger(count) || count < 0 || count > 1_000_000) return null;
      safeSummary[key] = count as number;
    }
  }

  return {
    protocolVersion: 'chaptera.reader-compatibility-report.v1',
    sourceSha256: source.source_sha256,
    state: source.state,
    ...(summary != null ? { contentSummary: safeSummary } : {}),
    limitations,
    outputRoutes: {
      readOnlyPreview: routes.read_only_preview as CanonicalOutputRoute,
      salvageRecovery: routes.salvage_recovery as CanonicalOutputRoute,
      editableIdml: 'not_verified',
      editableOdg: 'not_verified',
    },
    recommendedNextStep: nextByState[source.state],
  };
}
