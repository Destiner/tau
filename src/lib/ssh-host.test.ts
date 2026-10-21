import { describe, expect, it } from 'vitest';

import sshDisplayHost from './ssh-host';

describe('sshDisplayHost', () => {
  it.each([
    ['build-box', 'build-box'],
    ['deploy@build.example.test', 'build.example.test'],
    ['ssh deploy@build.example.test', 'build.example.test'],
    ['/usr/local/bin/ssh deploy@build.example.test', 'build.example.test'],
    ['ssh -p 2222 deploy@build.example.test', 'build.example.test'],
    ['ssh deploy@build.example.test -p 2222', 'build.example.test'],
    ['ssh -vp 2222 deploy@build.example.test', 'build.example.test'],
    ['ssh -p2222 deploy@build.example.test', 'build.example.test'],
    ['ssh -i~/.ssh/id_ed25519 deploy@build.example.test', 'build.example.test'],
    [
      'ssh -o "IdentityFile=~/.ssh/private key" deploy@build.example.test',
      'build.example.test',
    ],
    ['ssh -J "jump host" deploy@build.example.test', 'build.example.test'],
    ['ssh -- deploy@build.example.test', 'build.example.test'],
    ['ssh deploy@192.0.2.1', '192.0.2.1'],
    ['ssh deploy@[2001:db8::1]', '2001:db8::1'],
    ['ssh 2001:db8::1', '2001:db8::1'],
  ])('extracts only the safe host from %s', (connection, host) => {
    expect(sshDisplayHost(connection)).toBe(host);
  });

  it.each([
    undefined,
    '',
    '   ',
    'ssh',
    '/usr/bin/ssh',
    'ssh -p',
    'ssh -o "IdentityFile=~/.ssh/private key"',
    'ssh build.example.test uptime',
    'ssh -- build.example.test uptime',
    'ssh -- -p 2222',
    'ssh --bad-option build.example.test',
    'ssh deploy:password@build.example.test',
    'ssh deploy@build.example.test@other.example.test',
    'ssh deploy@/Users/tau/.ssh/id_ed25519',
    'ssh deploy@host:2222',
    'ssh deploy@999.999.999.999',
    'ssh deploy@2001:db8:::1',
    'ssh "unterminated',
  ])('rejects unparseable or unsafe connection strings: %s', (connection) => {
    expect(sshDisplayHost(connection)).toBeUndefined();
  });
});
