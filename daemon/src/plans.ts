/**
 * Minimal async SQLite surface needed here, matching pi-durable's own `SqliteExecutor` shape
 * (`dist/storage/sqlite/database.d.ts`) so the title store can share a `NodeSqliteDatabase`
 * connection opened via `openNodeSqliteDatabase` without re-importing `node:sqlite` directly.
 */
export type SqliteLike = {
  exec(sql: string): Promise<void>;
  run(sql: string, ...params: (string | number)[]): Promise<void>;
  get<T extends object>(sql: string, ...params: (string | number)[]): Promise<T | undefined>;
};

const TITLE_MAX_LENGTH = 60;

export type TitleOptions = {
  readonly isRoot?: boolean;
};

/**
 * Derive a conversation title from its first user message. The root conversation is always
 * "Lead". An empty/missing message falls back to "Untitled". A message longer than
 * `TITLE_MAX_LENGTH` characters is truncated at the last word boundary within budget and
 * suffixed with an ellipsis.
 */
export function deriveTitle(firstMessage: string | undefined, options: TitleOptions = {}): string {
  if (options.isRoot) return "Lead";
  if (!firstMessage) return "Untitled";

  const trimmed = firstMessage.trim();
  if (!trimmed) return "Untitled";
  if (trimmed.length <= TITLE_MAX_LENGTH) return trimmed;

  const slice = trimmed.slice(0, TITLE_MAX_LENGTH);
  const lastSpace = slice.lastIndexOf(" ");
  const cut = lastSpace > 0 ? slice.slice(0, lastSpace) : slice;
  return `${cut}\u2026`;
}

/** Shape of one `scanConversations` item, as returned by Pi Durable's storage layer. */
export type RawConversationRecord = {
  readonly id: number;
  readonly parent?: {
    readonly conversationId: number;
    readonly at: number;
  };
};

export type ConversationNode = {
  id: number;
  parentId: number | null;
  at: number | null;
  title: string;
};

/**
 * Map raw `scanConversations` items into the API's `ConversationNode` shape. Pure function so it
 * is testable without a storage harness. `titleFor` resolves the title for each id (derived or
 * overridden).
 */
export function shapeConversationTree(
  items: readonly RawConversationRecord[],
  titleFor: (id: number) => string,
): ConversationNode[] {
  return items.map((item) => ({
    id: item.id,
    parentId: item.parent?.conversationId ?? null,
    at: item.parent?.at ?? null,
    title: titleFor(item.id),
  }));
}

export type TitleStore = {
  get(id: number): Promise<string | undefined>;
  set(id: number, title: string): Promise<void>;
};

/**
 * Small SQLite-backed override table for user-supplied conversation titles, living in the same
 * database file as Pi Durable's own tables (opened as a second connection via
 * `openNodeSqliteDatabase`). Durable has no title field; we don't write into its schema.
 */
export async function openTitleStore(db: SqliteLike): Promise<TitleStore> {
  await db.exec("CREATE TABLE IF NOT EXISTS conversation_titles (id INTEGER PRIMARY KEY, title TEXT NOT NULL)");

  return {
    async get(id: number): Promise<string | undefined> {
      const row = await db.get<{ title: string }>(
        "SELECT title FROM conversation_titles WHERE id = ?",
        id,
      );
      return row?.title;
    },
    async set(id: number, title: string): Promise<void> {
      await db.run(
        "INSERT INTO conversation_titles (id, title) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET title = excluded.title",
        id,
        title,
      );
    },
  };
}

/** Resolve the `conversation` query param to a conversation id, defaulting to the root. */
export function resolveConversationId(url: URL, rootId: number): number {
  const raw = url.searchParams.get("conversation");
  if (raw === null) return rootId;
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : rootId;
}
