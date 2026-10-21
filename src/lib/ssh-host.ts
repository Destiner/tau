const SSH_OPTIONS_WITH_ARGUMENTS = new Set([
  'B',
  'b',
  'c',
  'D',
  'E',
  'e',
  'F',
  'I',
  'i',
  'J',
  'L',
  'l',
  'm',
  'O',
  'o',
  'P',
  'p',
  'Q',
  'R',
  'S',
  'W',
  'w',
]);

/**
 * Returns the destination host from a Tau SSH connection string when it is
 * safe to include in UI copy. This mirrors the accepted short-option forms in
 * the Rust SSH parser without returning any option values or user information.
 */
function sshDisplayHost(
  connectionString: string | undefined,
): string | undefined {
  if (!connectionString) return undefined;

  const words = shellWords(connectionString.trim());
  if (!words?.length) return undefined;

  if (isSshExecutable(words[0])) words.shift();

  let destination: string | undefined;
  for (let index = 0; index < words.length; index += 1) {
    const word = words[index]!;
    if (word === '--') {
      if (destination || index + 2 !== words.length) return undefined;
      destination = words[index + 1];
      break;
    }

    if (word.startsWith('-') && word.length > 1) {
      const option = word.slice(1);
      if (option.startsWith('-')) return undefined;

      let takesSeparateArgument = false;
      for (let optionIndex = 0; optionIndex < option.length; optionIndex += 1) {
        if (SSH_OPTIONS_WITH_ARGUMENTS.has(option[optionIndex]!)) {
          takesSeparateArgument = optionIndex === option.length - 1;
          break;
        }
      }
      if (takesSeparateArgument && ++index >= words.length) return undefined;
      continue;
    }

    if (destination) return undefined;
    destination = word;
  }

  return destinationHost(destination);
}

function shellWords(value: string): string[] | undefined {
  const words: string[] = [];
  let word = '';
  let quoted = false;
  let quote: '"' | "'" | undefined;

  for (let index = 0; index < value.length; index += 1) {
    const character = value[index]!;
    if (quote) {
      if (character === quote) {
        quote = undefined;
      } else if (character === '\\' && index + 1 < value.length) {
        word += value[++index];
      } else {
        word += character;
      }
      continue;
    }

    if (character === '"' || character === "'") {
      quote = character;
      quoted = true;
    } else if (/\s/.test(character)) {
      if (quoted || word) words.push(word);
      word = '';
      quoted = false;
    } else if (character === '\\' && index + 1 < value.length) {
      word += value[++index];
      quoted = true;
    } else {
      word += character;
    }
  }

  if (quote) return undefined;
  if (quoted || word) words.push(word);
  return words;
}

function isSshExecutable(word: string | undefined): boolean {
  return word === 'ssh' || word?.split('/').at(-1) === 'ssh';
}

function destinationHost(destination: string | undefined): string | undefined {
  if (!destination) return undefined;

  const at = destination.indexOf('@');
  const user = at === -1 ? undefined : destination.slice(0, at);
  let host = at === -1 ? destination : destination.slice(at + 1);
  if (
    (user !== undefined && (!user || !/^[A-Za-z0-9._-]+$/.test(user))) ||
    host.includes('@')
  ) {
    return undefined;
  }

  if (host.startsWith('[') && host.endsWith(']')) host = host.slice(1, -1);
  return isSafeHost(host) ? host : undefined;
}

function isSafeHost(host: string): boolean {
  if (!host || host.length > 253 || /[\s/\\]/.test(host)) return false;
  if (/^\d+(?:\.\d+){3}$/.test(host)) return isIpv4Address(host);
  return isIpv4Address(host) || isIpv6Address(host) || isHostname(host);
}

function isHostname(host: string): boolean {
  return host
    .split('.')
    .every((label) =>
      /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label),
    );
}

function isIpv4Address(value: string): boolean {
  const octets = value.split('.');
  return (
    octets.length === 4 &&
    octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255)
  );
}

function isIpv6Address(value: string): boolean {
  if (!value.includes(':') || value.split('::').length > 2) return false;

  const [left = '', right = ''] = value.split('::');
  const parts = [
    ...(left ? left.split(':') : []),
    ...(right ? right.split(':') : []),
  ];
  const ipv4 = parts.at(-1);
  const ipv4Groups = ipv4 && isIpv4Address(ipv4) ? 2 : 0;
  if (ipv4Groups) parts.pop();
  if (!parts.every((part) => /^[0-9A-Fa-f]{1,4}$/.test(part))) return false;

  const groups = parts.length + ipv4Groups;
  return value.includes('::') ? groups < 8 : groups === 8;
}

export default sshDisplayHost;
