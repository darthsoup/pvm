"""Isolated generated-hook and PTY checks. Never switch real PHP or services."""
import errno
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest
import fcntl

BINARY = str(Path(sys.argv.pop(1)).resolve())
SHELLS = {name: shutil.which(name) for name in ('bash', 'zsh', 'fish')}
for name in os.environ.get('PVM_REQUIRED_SHELLS', '').split(','):
    if name and not SHELLS.get(name):
        raise RuntimeError('Required shell is missing: ' + name)


def read_terminal(fd):
    try:
        return os.read(fd, 65536)
    except OSError as error:
        # Linux reports PTY hangup as EIO; macOS returns an empty read.
        if error.errno == errno.EIO:
            return b''
        raise


class ShellTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='pvm-shell-tests-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / 'home'
        (self.home / '.pvm').mkdir(parents=True)
        self.state('one', 1)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.project = self.root / 'project'
        (self.project / 'a').mkdir(parents=True)
        (self.project / 'b').mkdir()
        (self.project / '.pvmrc').write_text(' 8.3 \n')
        self.empty = self.root / 'empty'
        self.empty.mkdir()
        self.log = self.root / 'calls'
        self.stub = self.bin / 'pvm'
        self.stub.write_text('''#!/bin/sh
[ "$1" = auto ] && [ "$2" = --hook ] || exit 99
printf 'call\\n' >> "$HOME/calls"
if [ -e "$HOME/fail" ]; then printf 'activation failed\\n' >&2; exit 1; fi
IFS="$(printf '\\t')" read -r token enabled < "$HOME/.pvm/hook-state"
printf '%s\\t8.3\\t0\\n' "$token"
''')
        self.stub.chmod(0o755)
        self.env = dict(os.environ, HOME=str(self.home), PATH=str(self.bin) + os.pathsep + os.environ.get('PATH', ''), TERM='xterm', NO_COLOR='1')

    def state(self, token, enabled):
        (self.home / '.pvm/hook-state').write_text('{}\t{}\n'.format(token, enabled))

    def run_shell(self, shell, actions):
        hook = self.root / ('hook.' + shell)
        hook.write_bytes(subprocess.check_output([BINARY, 'init', '--shell', shell]))
        # Actions are fixed test strings; quote filesystem paths for the shell.
        import shlex
        quote = shlex.quote
        lines = ['source ' + quote(str(hook))]
        for action in actions:
            if action.startswith('cd '):
                lines.append('cd ' + quote(str(self.project / action[3:])))
                if shell == 'bash':
                    lines.append('_pvm_auto_switch')
            elif action == 'invalidate':
                lines += ["printf 'two\\t1\\n' > \"$HOME/.pvm/hook-state\"", '_pvm_auto_switch']
            elif action == 'disable':
                lines += ["printf 'two\\t0\\n' > \"$HOME/.pvm/hook-state\"", '_pvm_auto_switch']
            elif action == 'recover':
                lines += ['rm "$HOME/fail"', '_pvm_auto_switch']
            elif action == 'multiline':
                lines += ["printf '8.3\\ninvalid\\n' > " + quote(str(self.project / '.pvmrc')), '_pvm_auto_switch']
            elif action == 'reload':
                lines.append('source ' + quote(str(hook)))
            else:
                lines.append(action)
        flags = {'bash': ['--noprofile', '--norc'], 'zsh': ['-f'], 'fish': ['--no-config']}[shell]
        result = subprocess.run([SHELLS[shell], *flags, '-c', '\n'.join(lines)], cwd=self.empty, env=self.env, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        count = len((self.home / 'calls').read_text().splitlines()) if (self.home / 'calls').exists() else 0
        return result, count

    def each_shell(self, actions, calls, errors=None):
        for shell, executable in SHELLS.items():
            if executable:
                with self.subTest(shell=shell):
                    (self.home / 'calls').unlink(missing_ok=True)
                    self.state('one', 1)
                    result, count = self.run_shell(shell, actions)
                    self.assertEqual(count, calls, result.stderr)
                    if errors is not None:
                        self.assertEqual(result.stderr.count('activation failed'), errors)

    def test_cached_subdirectories_and_shared_invalidation(self):
        self.each_shell(['cd a', 'cd b', 'invalidate', 'cd a'], 2)

    def test_disabled_hook_does_not_call_backend(self):
        self.each_shell(['disable', 'cd a'], 0)

    def test_failures_retry_but_only_warn_once_and_recover(self):
        for shell, executable in SHELLS.items():
            if executable:
                with self.subTest(shell=shell):
                    (self.home / 'calls').unlink(missing_ok=True)
                    (self.home / 'fail').touch()
                    result, count = self.run_shell(shell, ['cd a', 'cd b', 'recover', 'cd a'])
                    self.assertEqual(count, 3, result.stderr)
                    self.assertEqual(result.stderr.count('activation failed'), 1)

    def test_multiline_change_invalidates_cached_request(self):
        for shell, executable in SHELLS.items():
            if executable:
                with self.subTest(shell=shell):
                    (self.home / 'calls').unlink(missing_ok=True)
                    (self.project / '.pvmrc').write_text('8.3\n')
                    _, count = self.run_shell(shell, ['cd a', 'multiline'])
                    self.assertEqual(count, 2)

    def test_empty_nearest_file_stops_parent_search(self):
        (self.project / 'a/.php-version').write_text(' \n')
        self.each_shell(['cd a'], 0)

    def test_hooks_do_not_inherit_parent_cache(self):
        self.env.update(PVM_PHP_VERSION='8.3', _PVM_SOURCE=str(self.project / '.pvmrc'), _PVM_TOKEN='one')
        self.each_shell(['cd a'], 1)

    def test_bash_errexit_survives_activation_failure(self):
        (self.home / 'fail').touch()
        result, count = self.run_shell('bash', ['set -e', 'cd a', 'cd b', "printf 'survived\\n'"])
        self.assertEqual(count, 2)
        self.assertIn('survived', result.stdout)

    def test_reload_is_idempotent(self):
        self.each_shell(['reload', 'cd a', 'cd b'], 1)


class PtyTests(unittest.TestCase):
    def run_pty(self, keys, arguments=('use',), columns=100, prompt=b'Select global PHP', stdout_terminal=False):
        with tempfile.TemporaryDirectory(prefix='pvm-pty-') as directory:
            root = Path(directory)
            (root / 'bin').mkdir()
            php = root / 'bin/php'
            php.write_text("#!/bin/sh\nprintf 'PHP 99.1.2 (cli)\\n'\n")
            php.chmod(0o755)
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, columns, 0, 0))
            before = termios.tcgetattr(slave)
            # Set a controlling terminal, since Console reads /dev/tty.
            def setup():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)
            env = dict(os.environ, HOME=str(root), PATH=str(root / 'bin'), TERM='xterm', NO_COLOR='1')
            marker = root / 'finished'
            launcher = "import os,subprocess,sys,time; code=subprocess.call(sys.argv[2:]); open(sys.argv[1],'w').write(str(code)); exec(\"while not os.path.exists(sys.argv[1]+'.ack'): time.sleep(.01)\"); sys.exit(code)"
            process = subprocess.Popen([sys.executable, '-c', launcher, str(marker), BINARY, *arguments], cwd=root, env=env, stdin=slave, stderr=slave, stdout=slave if stdout_terminal else subprocess.PIPE, preexec_fn=setup)
            data = b''
            deadline = time.monotonic() + 10
            sent = not keys
            try:
                while time.monotonic() < deadline:
                    if select.select([master], [], [], 0.05)[0]:
                        chunk = read_terminal(master)
                        if not chunk:
                            break
                        data += chunk
                    if not sent and prompt in data and not (termios.tcgetattr(slave)[3] & termios.ICANON):
                        os.write(master, keys)
                        sent = True
                    if marker.exists() and marker.read_text():
                        break
                else:
                    process.kill()
                    self.fail('PTY command timed out: ' + repr(data))
                code = int(marker.read_text())
                after = termios.tcgetattr(slave)
                # macOS may set its internal pending-input bit after a read.
                before[3] &= ~getattr(termios, 'PENDIN', 0)
                after[3] &= ~getattr(termios, 'PENDIN', 0)
                self.assertEqual(before, after, 'terminal flags not restored')
                Path(str(marker) + '.ack').touch()
                process.wait(timeout=2)
                while select.select([master], [], [], 0)[0]:
                    chunk = read_terminal(master)
                    if not chunk:
                        break
                    data += chunk
                stdout = process.stdout.read() if process.stdout is not None else b''
                return code, stdout, data
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                if process.stdout is not None:
                    process.stdout.close()
                os.close(master)
                os.close(slave)

    def test_cancel_returns_success_and_restores_cursor(self):
        code, stdout, stderr = self.run_pty(b'q')
        self.assertEqual(code, 0, stderr)
        self.assertEqual(stdout, b'')
        self.assertIn(b'Selection cancelled', stderr)
        self.assertIn(b'\x1b[?25h', stderr)

    def test_interrupt_returns_130_and_restores_cursor(self):
        code, stdout, stderr = self.run_pty(b'\x03')
        self.assertEqual(code, 130, stderr)
        self.assertEqual(stdout, b'')
        self.assertIn(b'\x1b[?25h', stderr)

    def test_enter_confirms_active_version(self):
        code, stdout, stderr = self.run_pty(b'\r')
        self.assertEqual(code, 0, stderr)
        self.assertIn(b'Already using PHP 99.1', stderr)
        self.assertEqual(stdout, b'')

    def test_narrow_list_uses_stacked_layout(self):
        code, stdout, terminal = self.run_pty(b'', ('list', '--color', 'never'), columns=60, stdout_terminal=True)
        self.assertEqual(code, 0, terminal)
        self.assertIn(b'99.1  active', terminal)
        self.assertNotIn(b'VERSION', terminal)
        self.assertNotIn(b'\x1b[', terminal)


if __name__ == '__main__':
    unittest.main()
