"""Synthetic tests for the narrowly scoped shared-v24 recovery tool."""
import importlib.util
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "recover-shared-v23.py"


class SharedRecoveryTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("recover_shared_v23", SCRIPT)
        self.recovery = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.recovery)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.reference = self.root / "reference.db"
        self.database = self.root / "current.db"
        self.backups = self.root / "backups"
        with closing(sqlite3.connect(self.reference)) as conn:
            conn.executescript(
                """CREATE TABLE mcp_servers (
                    id TEXT PRIMARY KEY, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
                    enabled_pi BOOLEAN NOT NULL DEFAULT 0,
                    enabled_agy BOOLEAN NOT NULL DEFAULT 0,
                    enabled_kimi BOOLEAN NOT NULL DEFAULT 0,
                    enabled_omp BOOLEAN NOT NULL DEFAULT 0);
                CREATE TABLE skills (
                    id TEXT PRIMARY KEY, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
                    enabled_agy BOOLEAN NOT NULL DEFAULT 0,
                    enabled_kimi BOOLEAN NOT NULL DEFAULT 0);
                CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
                INSERT INTO mcp_servers (id,enabled_omp) VALUES ('synthetic',1);
                INSERT INTO skills (id,enabled_kimi) VALUES ('synthetic',1);
                INSERT INTO settings VALUES ('synthetic','before');
                PRAGMA user_version=23;"""
            )
            with closing(sqlite3.connect(self.database)) as target:
                conn.backup(target)
        with closing(sqlite3.connect(self.database)) as conn:
            conn.executescript(
                """ALTER TABLE mcp_servers ADD COLUMN enabled_dsh BOOLEAN NOT NULL DEFAULT 0;
                ALTER TABLE skills ADD COLUMN enabled_dsh BOOLEAN NOT NULL DEFAULT 0;
                UPDATE settings SET value='after';
                PRAGMA user_version=24;"""
            )

    def run_recovery(self, apply=False):
        return self.recovery.recover(
            self.database, self.reference, self.backups, apply=apply
        )

    def test_dry_run_does_not_write_or_create_a_backup(self):
        before = self.database.read_bytes()
        self.assertIsNone(self.run_recovery())
        self.assertEqual(self.database.read_bytes(), before)
        self.assertFalse(self.backups.exists())

    def test_recovery_keeps_current_rows_and_a_private_v24_backup(self):
        backup = self.run_recovery(apply=True)
        with closing(sqlite3.connect(self.database)) as conn:
            self.assertEqual(conn.execute("PRAGMA user_version").fetchone()[0], 23)
            self.assertEqual(conn.execute("SELECT value FROM settings").fetchone()[0], "after")
            self.assertEqual(conn.execute("SELECT enabled_omp FROM mcp_servers").fetchone()[0], 1)
            self.assertEqual(conn.execute("PRAGMA integrity_check").fetchall(), [("ok",)])
        with closing(sqlite3.connect(backup)) as conn:
            self.assertEqual(conn.execute("PRAGMA user_version").fetchone()[0], 24)
            self.assertEqual(conn.execute("SELECT value FROM settings").fetchone()[0], "after")
        self.assertEqual(backup.stat().st_mode & 0o777, 0o600)

    def test_unknown_or_nondefault_schema_is_rejected_without_changes(self):
        for sql in [
            "UPDATE mcp_servers SET enabled_dsh=1;",
            "UPDATE skills SET enabled_dsh=1;",
            "CREATE TABLE unknown (value TEXT);",
            "CREATE INDEX unknown ON skills(enabled_dsh);",
            "ALTER TABLE settings ADD COLUMN unknown TEXT;",
            "PRAGMA user_version=25;",
        ]:
            with self.subTest(sql=sql):
                pristine = self.database.read_bytes()
                with closing(sqlite3.connect(self.database)) as conn:
                    conn.executescript(sql)
                before = self.database.read_bytes()
                with self.assertRaises(self.recovery.RecoveryError):
                    self.run_recovery(apply=True)
                self.assertEqual(self.database.read_bytes(), before)
                self.database.write_bytes(pristine)

    def test_backup_failure_leaves_the_live_database_unchanged(self):
        self.backups.write_text("synthetic blocker")
        before = self.database.read_bytes()
        with self.assertRaises((self.recovery.RecoveryError, OSError)):
            self.run_recovery(apply=True)
        self.assertEqual(self.database.read_bytes(), before)

    def test_missing_reference_is_not_created(self):
        self.reference.unlink()
        with self.assertRaises(self.recovery.RecoveryError):
            self.run_recovery(apply=True)
        self.assertFalse(self.reference.exists())


if __name__ == "__main__":
    unittest.main()
