"""Exercise the real confirmation prompt from its unit-test subprocess."""
from shells import PtyTests

code, stdout, terminal = PtyTests().run_pty(
    b'\r', ('--exact', 'prompts::tests::confirmation_defaults_to_no', '--nocapture'),
    prompt=b'Restart fixture?',
)
assert code == 0, (stdout, terminal)
assert b'[y/N]' in terminal, terminal
