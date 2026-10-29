"""Reverse only the verified accidental v23-to-v24 shared schema upgrade."""
import argparse
from contextlib import closing
import hashlib
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile


class RecoveryError(Exception):
    pass


def schema(conn):
    return conn.execute(
        "SELECT type,name,tbl_name,sql FROM sqlite_master "
        "WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
    ).fetchall()


def content_digest(conn):
    digest = hashlib.sha256()
    for statement in conn.iterdump():
        digest.update(statement.encode("utf-8"))
        digest.update(b"\n")
    digest.update(str(conn.execute("PRAGMA user_version").fetchone()[0]).encode("ascii"))
    return digest.digest()


def validate(source, reference):
    if reference.execute("PRAGMA user_version").fetchone()[0] != 23:
        raise RecoveryError("The reference must be a verified shared v23 backup.")
    if source.execute("PRAGMA user_version").fetchone()[0] != 24:
        raise RecoveryError("Recovery applies only to the accidental shared v24 format.")
    if source.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
        raise RecoveryError("Integrity validation failed; no changes were made.")
    for table in ["mcp_servers", "skills"]:
        required = {"enabled_mcode", "enabled_agy", "enabled_kimi"}
        if table == "mcp_servers":
            required.update(["enabled_pi", "enabled_omp"])
        columns = reference.execute(f"PRAGMA table_info({table})").fetchall()
        if not required.issubset({row[1] for row in columns}):
            raise RecoveryError("The reference does not match the released v23 schema.")
        current = source.execute(f"PRAGMA table_info({table})").fetchall()
        if (
            current[:-1] != columns
            or len(current) != len(columns) + 1
            or current[-1][1:] != ("enabled_dsh", "BOOLEAN", 1, "0", 0)
        ):
            raise RecoveryError("The column changes do not match the known upgrade.")
        if source.execute(
            f"SELECT EXISTS(SELECT 1 FROM {table} WHERE enabled_dsh IS NOT 0)"
        ).fetchone()[0]:
            raise RecoveryError("Nondefault v24 data exists; automatic recovery is unsafe.")
    # Prove the entire reversed schema matches the user's verified v23 backup.
    with closing(sqlite3.connect(":memory:")) as projected:
        source.backup(projected)
        try:
            for table in ["mcp_servers", "skills"]:
                projected.execute(f"ALTER TABLE {table} DROP COLUMN enabled_dsh")
        except sqlite3.Error:
            raise RecoveryError("Dependent schema objects prevent safe reversal.") from None
        if schema(projected) != schema(reference):
            raise RecoveryError("Other schema changes exist; automatic recovery is unsafe.")


def open_existing(path, mode):
    if not path.is_file() or path.is_symlink():
        raise RecoveryError("An existing regular database file is required.")
    return sqlite3.connect(path.resolve().as_uri() + f"?mode={mode}", uri=True, timeout=2)


def recover(database, reference, backup_dir, *, apply=False):
    database, reference, backup_dir = map(Path, (database, reference, backup_dir))
    if database.resolve() == reference.resolve():
        raise RecoveryError("The current database and reference must be different files.")
    with closing(open_existing(database, "ro")) as source, closing(open_existing(reference, "ro")) as ref:
        validate(source, ref)
        if not apply:
            return None
        with closing(sqlite3.connect(":memory:")) as snapshot:
            source.backup(snapshot)
            validate(snapshot, ref)
            backup_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
            # A private, exclusive backup is durable before touching the current file.
            fd, name = tempfile.mkstemp(prefix="cc-switch-before-v23-recovery-", suffix=".db", dir=backup_dir)
            backup = Path(name)
            try:
                with closing(sqlite3.connect(backup)) as target:
                    snapshot.backup(target)
                    if target.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
                        raise RecoveryError("Recovery backup validation failed.")
                os.fsync(fd)
            finally:
                os.close(fd)
            dir_fd = os.open(backup_dir, os.O_RDONLY)
            try:
                os.fsync(dir_fd)
            finally:
                os.close(dir_fd)
            # Refuse any changes made since the validated snapshot was taken.
            with closing(open_existing(database, "rw")) as live, live:
                live.execute("BEGIN EXCLUSIVE")
                if content_digest(live) != content_digest(snapshot):
                    raise RecoveryError("The database changed during recovery; retry with both apps closed.")
                for table in ["mcp_servers", "skills"]:
                    live.execute(f"ALTER TABLE {table} DROP COLUMN enabled_dsh")
                if schema(live) != schema(ref):
                    raise RecoveryError("Reversed schema validation failed; changes were rolled back.")
                live.execute("PRAGMA user_version=23")
                if live.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
                    raise RecoveryError("Final integrity validation failed; changes were rolled back.")
                if live.execute("PRAGMA foreign_key_check").fetchone() is not None:
                    raise RecoveryError("Foreign-key validation failed; changes were rolled back.")
    return backup


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--backup-dir", required=True, type=Path)
    parser.add_argument("--apply", action="store_true", help="Write after validation; default is read-only.")
    args = parser.parse_args()
    try:
        if args.apply:
            commands = subprocess.check_output(["ps", "-Ao", "comm="], text=True).splitlines()
            if any(Path(command.strip()).name.lower() in {"varyai", "cc-switch"} for command in commands):
                raise RecoveryError("Close VaryAI and CC Switch before applying recovery.")
        backup = recover(args.database, args.reference, args.backup_dir, apply=args.apply)
    except RecoveryError as error:
        parser.exit(1, f"Recovery stopped: {error}\n")
    except (OSError, sqlite3.Error):
        parser.exit(1, "Recovery stopped because a file, database, or lock operation failed; no uncommitted changes were retained.\n")
    if backup:
        print(f"Recovered the released shared v23 schema. Current rows were retained. Backup: {backup}")
    else:
        print("Read-only validation passed. Only the known default-valued v24 columns differ from the v23 reference.")


if __name__ == "__main__":
    main()
