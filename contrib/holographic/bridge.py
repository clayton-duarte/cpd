#!/usr/bin/env python3
"""cpd's JSON-lines bridge to the holographic memory engine.

One request per line on stdin, one response per line on stdout, in order.
Diagnostics go to stderr; standard output carries the protocol and nothing else.
The process exits on EOF. See docs/research/context-holographic.md, `## Bridge`.
"""

import argparse
import importlib.metadata
import json
import os
import sqlite3
import sys


def response(**fields):
    return fields


def error(kind, message):
    return {"ok": False, "error": kind, "message": str(message)}


def content_of(fact):
    parts = []
    for key in ("topic", "short", "text"):
        value = fact.get(key)
        if value is not None and str(value) != "":
            parts.append(str(value))
    return " ".join(parts)


class Bridge:
    def __init__(self, db_path):
        self.db_path = str(db_path)
        parent = os.path.dirname(os.path.abspath(self.db_path))
        if parent:
            os.makedirs(parent, exist_ok=True)
        self.store = None
        self.retriever = None

    def open(self):
        if self.store is None:
            import holographic_memory

            self.store = holographic_memory.MemoryStore(self.db_path)
            self.retriever = holographic_memory.FactRetriever(self.store)
            self.store._conn.execute(
                "CREATE TABLE IF NOT EXISTS cpd_watermark ("
                "key TEXT PRIMARY KEY, value TEXT)"
            )
            self.store._conn.commit()
        return self.store

    def watermark(self):
        store = self.open()
        values = {}
        for row in store._conn.execute("SELECT key, value FROM cpd_watermark"):
            values[row[0]] = row[1]
        indexed_seq = int(values.get("indexed_seq", "0") or 0)
        curated_at = values.get("curated_at")
        return indexed_seq, curated_at

    def set_watermark(self, indexed_seq=None, curated_at=None):
        store = self.open()
        if indexed_seq is not None:
            store._conn.execute(
                "INSERT INTO cpd_watermark (key, value) VALUES ('indexed_seq', ?)"
                " ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (str(indexed_seq),),
            )
        if curated_at is not None:
            store._conn.execute(
                "INSERT INTO cpd_watermark (key, value) VALUES ('curated_at', ?)"
                " ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (str(curated_at),),
            )
        store._conn.commit()

    def ping(self, request):
        import holographic_memory

        version = getattr(holographic_memory, "__version__", None)
        if version is None:
            try:
                version = importlib.metadata.version("holographic-memory")
            except importlib.metadata.PackageNotFoundError:
                version = "unknown"
        return response(ok=True, engine="holographic", version=str(version), pid=os.getpid())

    def stats(self, request):
        store = self.open()
        facts = store._conn.execute("SELECT count(*) FROM facts").fetchone()[0]
        indexed_seq, curated_at = self.watermark()
        return response(ok=True, facts=facts, indexed_seq=indexed_seq, curated_at=curated_at)

    def reset(self, request):
        store = self.open()
        store._conn.execute("DELETE FROM facts")
        store._conn.execute("DELETE FROM cpd_watermark")
        store._conn.commit()
        return response(ok=True)

    def index(self, request):
        store = self.open()
        facts = request.get("facts") or []
        inserted = 0
        updated = 0
        max_seq = None
        for fact in facts:
            record = str(fact.get("record", ""))
            content = content_of(fact)
            existing = store._conn.execute(
                "SELECT fact_id FROM facts WHERE category = ? LIMIT 1", (record,)
            ).fetchone()
            if existing is not None:
                store.update_fact(
                    existing[0], content=content, category=record, tags=""
                )
                updated += 1
            else:
                added = store.add_facts_batch([(content, record, "")])
                if added:
                    inserted += 1
                else:
                    # An exact duplicate content is skipped by the batch; the
                    # record still gets exactly one fact, marked with its id.
                    added = store.add_facts_batch([(content + " [" + record + "]", record, "")])
                    if added:
                        inserted += 1
                    elif store._conn.execute(
                        "SELECT 1 FROM facts WHERE category = ? LIMIT 1", (record,)
                    ).fetchone() is None:
                        raise RuntimeError(
                            "cannot index record %s: the engine holds its content under another record"
                            % record
                        )
            seq = fact.get("seq")
            if isinstance(seq, int) and (max_seq is None or seq > max_seq):
                max_seq = seq
        if max_seq is not None:
            self.set_watermark(indexed_seq=max_seq)
        self.set_watermark(
            indexed_seq=request.get("indexed_seq"),
            curated_at=request.get("curated_at"),
        )
        return response(ok=True, inserted=inserted, updated=updated)

    def search(self, request):
        store = self.open()
        query = request.get("query")
        if not isinstance(query, str):
            return error("bad_cmd", "search needs a text `query`")
        limit = request.get("limit", 20)
        if not isinstance(limit, int) or isinstance(limit, bool) or limit < 0:
            return error("bad_cmd", "search needs a non-negative integer `limit`")
        try:
            # Probe the same MATCH the retriever runs: holographic's
            # _fts_candidates swallows the syntax error as no hits.
            store._conn.execute(
                "SELECT 1 FROM facts_fts WHERE facts_fts MATCH ? LIMIT 1", (query,)
            ).fetchall()
        except sqlite3.OperationalError as exc:
            return error("bad_query", exc)
        hits = self.retriever.search(query, limit=limit)
        return response(
            ok=True,
            hits=[
                {
                    "fact_id": hit["fact_id"],
                    "record": hit["category"],
                    "score": hit["score"],
                }
                for hit in hits
            ],
        )

    def handle(self, line):
        try:
            request = json.loads(line)
        except json.JSONDecodeError as exc:
            return error("bad_json", exc)
        if not isinstance(request, dict):
            return error("bad_json", "a request is a JSON object")
        command = request.get("cmd")
        handler = {
            "ping": self.ping,
            "stats": self.stats,
            "reset": self.reset,
            "index": self.index,
            "search": self.search,
        }.get(command)
        if handler is None:
            return error("bad_cmd", "unknown cmd %r" % (command,))
        try:
            return handler(request)
        except sqlite3.OperationalError as exc:
            return error("engine_error", exc)
        except Exception as exc:  # engine failures surface in the protocol
            return error("engine_error", exc)


def main(argv=None):
    parser = argparse.ArgumentParser(description="cpd's holographic engine bridge")
    parser.add_argument("--db", required=True)
    args = parser.parse_args(argv)
    bridge = Bridge(args.db)
    for line in sys.stdin:
        line = line.strip()
        answer = bridge.handle(line)
        sys.stdout.write(json.dumps(answer, separators=(",", ":")) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
