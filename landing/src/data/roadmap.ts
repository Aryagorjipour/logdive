export const CURRENT_VERSION = '0.2.1';
export const IN_PROGRESS_VERSION = '0.3.0';
export const ROADMAP_UPDATED = '2026-06-01';

export type ItemKind = 'feature' | 'perf' | 'infra' | 'docs';

export interface RoadmapItem {
  title: string;
  kind: ItemKind;
  issue?: number;
  description?: string;
  note?: string;
  versionTarget?: string;
}

export interface ShippedRelease {
  version: string;
  date: string;
  highlights: string[];
}

export const roadmapNow: RoadmapItem[] = [
  {
    title: 'Parenthesised query expressions',
    kind: 'feature',
    issue: 51,
    description: 'Grouping with ( ) so NOT and OR compose cleanly without precedence surprises.',
  },
  {
    title: 'Distroless Docker image',
    kind: 'infra',
    description: 'Replace debian:bookworm-slim runtime with distroless/cc for a smaller attack surface.',
  },
  {
    title: 'Generated columns',
    kind: 'feature',
    description: 'Index derived fields at ingest time to accelerate repeated queries on computed values.',
  },
];

export const roadmapNext: RoadmapItem[] = [
  {
    title: 'Windows support for --follow mode',
    kind: 'infra',
    versionTarget: '0.4.0',
    description: 'Rotation and truncation detection on NTFS using ReadDirectoryChangesW.',
  },
  {
    title: 'Structured output formats: yaml, csv',
    kind: 'feature',
    issue: 39,
    versionTarget: '0.3.0',
  },
  {
    title: 'Configurable retention by source',
    kind: 'feature',
    issue: 44,
    versionTarget: '0.4.0',
    description: 'Let prune --older-than vary per source tag instead of one global cutoff.',
  },
];

export const roadmapLater: RoadmapItem[] = [
  {
    title: 'Authentication for the HTTP API',
    kind: 'infra',
    note: 'waiting on feedback',
    description: 'Currently a non-goal. Reconsidering only if the localhost-only stance is causing real pain.',
  },
  {
    title: 'Multi-file ingest with glob patterns',
    kind: 'feature',
    issue: 33,
    note: 'considering',
  },
  {
    title: 'Aggregations: count, distinct, group-by',
    kind: 'feature',
    issue: 21,
    note: 'considering',
  },
  {
    title: 'Browser-based query UI',
    kind: 'feature',
    note: 'needs spec',
    description: 'Listed for completeness; explicit v1 non-goal. Would need a separate crate and a real design pass.',
  },
];

export const shipped: ShippedRelease[] = [
  {
    version: '0.2.1',
    date: '2026-06-01',
    highlights: [
      'Security test suite: SQL injection, LIKE wildcard escaping, resource exhaustion (1k-disjunct OR, 10 MB line).',
      'Functional tests: proptest property-based, cross-format dedup, concurrent CLI ingest, parser edge cases, follow-mode, API integration, prune boundary.',
      'Supply-chain hardening: cargo-deny, SBOM via cargo-cyclonedx, daily audit CI, CI permissions: contents: read.',
      'Allocation improvements: LogEntry::with_tag takes &str, entry_to_json_string avoids clone per HTTP row.',
    ],
  },
  {
    version: '0.2.0',
    date: '2026-05-15',
    highlights: [
      'Added OR to the query language — (level=error OR level=warn) AND service=payments.',
      'Ingestion now accepts logfmt and plain-text lines alongside JSON.',
      'New --follow mode tails files with rotation and truncation detection.',
      'Introduced the prune subcommand for time-based retention with --older-than.',
      'HTTP API gained /version and /capabilities endpoints, plus configurable CORS.',
      'Docker image is now multi-stage and multi-arch, down to ~9 MB compressed.',
    ],
  },
  {
    version: '0.1.0',
    date: '2026-04-19',
    highlights: [
      'Initial release with ingest, query, and stats subcommands on the CLI.',
      'SQLite-backed local indexing with blake3 content hashing for dedup.',
      'Typed query language supporting AND, =, !=, >, <, contains, last, and since.',
      'Read-only HTTP server exposing /query as NDJSON and /stats as JSON.',
    ],
  },
];
