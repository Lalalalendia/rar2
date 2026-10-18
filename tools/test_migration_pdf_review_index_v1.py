"""Source-free HTML projection guards; no source PUB, font or PDF renderer."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import migration_operator_pdf_report_v1 as reporter
import migration_pdf_review_index_v1 as page


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class OfflineReviewIndexTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.pdf = self.root / 'pdf'
        self.html = self.root / 'review'
        self.pdf.mkdir()
        self.html.mkdir()
        self.name = 'fixture--123456789abc--1234567890abcdef.pdf'
        self.pdf_path = self.pdf / self.name
        self.pdf_path.write_bytes(b'%PDF-1.7\nsynthetic output')
        self.loss_text = self.pdf / (self.name + '.loss.txt')
        self.loss_text.write_text('synthetic loss', encoding='utf-8')
        self.good = {
            'source_relative_path': 'nested/issue.pub',
            'state': 'integrity_confirmed_review_required',
            'reason': 'visual_and_loss_review_required',
            'output_pdf': self.name,
            'pdf_sha256': sha(self.pdf_path.read_bytes()),
        }
        self.receipt = {
            'schema': page.SCHEMA,
            'claim_boundary': page.BOUNDARY,
            'source_receipt_sha256': sha(b'private manifest'),
            'total_sources': 1,
            'checkpoint_rows': 1,
            'counts': dict(
                integrity_confirmed_review_required=1,
                integrity_rejected=0,
                exception=0,
                not_yet_in_checkpoint=0,
            ),
            'entries': [self.good],
        }

    def render(self) -> str:
        return page.render_index(self.receipt, self.pdf, self.html)

    def test_original_pdf_and_loss_can_be_opened_locally(self):
        rendered = self.render()
        self.assertIn('Open PDF', rendered)
        self.assertIn('Loss report', rendered)
        self.assertIn('../pdf/', rendered)
        self.assertIn('Not approved for delivery', rendered)
        self.assertIn('No file is automatically approved', rendered)
        self.assertIn('not establish Microsoft Publisher layout fidelity', rendered)

    def test_no_remote_assets_js_or_inline_source_bytes(self):
        rendered = self.render()
        self.assertIn("default-src 'none'", rendered)
        self.assertNotIn('<script', rendered)
        self.assertNotIn('<iframe', rendered)
        self.assertNotIn('https://', rendered)
        self.assertNotIn('%PDF-1.7', rendered)
        self.assertNotIn('synthetic output', rendered)

    def test_untrusted_html_in_source_name_is_escaped(self):
        self.good['source_relative_path'] = 'invoice/<svg onload=alert(1)>.pub'
        rendered = self.render()
        self.assertNotIn('<svg onload=', rendered)
        self.assertIn('&lt;svg onload=alert(1)&gt;', rendered)
        self.assertNotIn('href="javascript:', rendered)

    def test_failed_source_never_links_to_unverified_output(self):
        self.good.update(state='exception', reason='converter_failed', output_pdf=self.name)
        self.receipt['counts'].update(integrity_confirmed_review_required=0, exception=1)
        rendered = self.render()
        self.assertIn('Conversion exception', rendered)
        self.assertIn('No verified PDF link', rendered)
        self.assertNotIn('Open PDF', rendered)
        self.assertNotIn('Loss report', rendered)

    def test_bad_integrity_never_links_to_output(self):
        self.good.update(state='integrity_rejected', reason='output_missing_or_hash_mismatch')
        self.receipt['counts'].update(integrity_confirmed_review_required=0, integrity_rejected=1)
        rendered = self.render()
        self.assertIn('Integrity rejected', rendered)
        self.assertNotIn('Open PDF', rendered)

    def test_pending_sources_are_visible_without_fabricated_row(self):
        self.receipt['total_sources'] = 2
        self.receipt['counts']['not_yet_in_checkpoint'] = 1
        rendered = self.render()
        self.assertIn('Not processed', rendered)
        self.assertIn('2 original candidates; 1 rows in this checkpoint', rendered)
        self.assertEqual(rendered.count('Open PDF'), 1)

    def test_forged_status_rejected(self):
        self.good['state'] = 'customer_accepted'
        with self.assertRaisesRegex(page.ReviewIndexRejected, 'unrecognized_review_state'):
            self.render()

    def test_missing_or_false_source_manifest_rejected(self):
        self.receipt['source_receipt_sha256'] = 'not a sha'
        with self.assertRaisesRegex(page.ReviewIndexRejected, 'missing_source_manifest_fingerprint'):
            self.render()

    def test_unbalanced_counts_fail_closed(self):
        self.receipt['counts']['exception'] = 1
        with self.assertRaisesRegex(page.ReviewIndexRejected, 'review_counts_do_not_cover_sources'):
            self.render()

    def test_windows_different_volume_falls_back_to_no_link(self):
        with patch.object(page.os.path, 'relpath', side_effect=ValueError('different drive')):
            rendered = self.render()
        self.assertNotIn('Open PDF', rendered)
        self.assertIn('Open from the PDF directory', rendered)
        self.assertNotIn(str(self.pdf), rendered)

    def test_missing_pdf_or_symlink_has_no_live_file_link(self):
        self.pdf_path.unlink()
        self.pdf_path.symlink_to(self.loss_text)
        rendered = self.render()
        self.assertNotIn('Open PDF', rendered)
        self.assertIn('Unavailable', rendered)

    def test_duplicate_source_identity_rejected(self):
        repeated = dict(self.good, source_relative_path='nested/ISSUE.pub')
        self.receipt['entries'].append(repeated)
        self.receipt['total_sources'] = 2
        self.receipt['checkpoint_rows'] = 2
        self.receipt['counts']['integrity_confirmed_review_required'] = 2
        with self.assertRaisesRegex(page.ReviewIndexRejected, 'duplicate_review_source'):
            self.render()

    def test_malicious_output_name_rejected_even_when_hashed(self):
        self.good['output_pdf'] = 'javascript:alert(1).pdf'
        with self.assertRaisesRegex(page.ReviewIndexRejected, 'invalid_review_pdf_name'):
            self.render()

    def test_atomic_report_writer_rejects_existing_html_symlink(self):
        receipt = dict(self.receipt)
        link = self.html / 'review-index.html'
        outside = self.root / 'outside.txt'
        outside.write_text('DO_NOT_OVERWRITE', encoding='utf-8')
        link.symlink_to(outside)
        with self.assertRaisesRegex(reporter.ReportRejected, 'unsafe_report_target'):
            reporter.write_reports(receipt, self.html, self.pdf)
        self.assertEqual(outside.read_text(encoding='utf-8'), 'DO_NOT_OVERWRITE')
        self.assertFalse((self.html / 'operator-report.json').exists())


if __name__ == '__main__':
    unittest.main()
