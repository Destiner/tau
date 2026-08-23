type PiErrorKind =
  | 'authentication'
  | 'credit'
  | 'rate-limit'
  | 'unavailable'
  | 'network'
  | 'context-limit'
  | 'model-unavailable'
  | 'invalid-request'
  | 'unknown';

interface PiErrorDescription {
  kind: PiErrorKind;
  label: string;
  message: string;
}

const errorMessages: Record<PiErrorKind, string> = {
  authentication:
    'The model provider could not authenticate this request. Check the provider credentials and try again.',
  credit:
    'The account has no available credit for this request. Add credit or choose another model, then try again.',
  'rate-limit':
    'The model provider is receiving too many requests. Wait a moment or choose another model, then try again.',
  unavailable:
    'The model provider is temporarily unavailable. Wait a moment or choose another model, then try again.',
  network:
    'The model provider could not be reached. Check the connection and try again.',
  'context-limit':
    'This conversation is too long for the selected model. Start a new session or choose a model with a larger context window.',
  'model-unavailable':
    'The selected model is unavailable to this account. Choose another model and try again.',
  'invalid-request':
    'The model provider could not process this request. Edit it or choose another model.',
  unknown: 'The reply failed. Try again or choose another model.',
};

function describePiError(raw: string): PiErrorDescription {
  const kind = classifyPiError(raw);
  return {
    kind,
    label: 'Reply Failed',
    message: errorMessages[kind],
  };
}

function classifyPiError(raw: string): PiErrorKind {
  const text = raw.toLocaleLowerCase();
  if (
    /\b401\b|unauthori[sz]ed|authentication|api[ _-]?key|credential/.test(text)
  ) {
    return 'authentication';
  }
  if (
    /insufficient[ _-]?quota|credit|billing|insufficient (?:balance|funds)|exceeded your current quota/.test(
      text,
    )
  ) {
    return 'credit';
  }
  if (/\b429\b|rate[ _-]?limit|too many requests/.test(text)) {
    return 'rate-limit';
  }
  if (/\b402\b|quota/.test(text)) {
    return 'credit';
  }
  if (
    /context (?:length|limit|window)|maximum context|max(?:imum)? tokens|conversation is too long/.test(
      text,
    )
  ) {
    return 'context-limit';
  }
  if (
    /model[^\n]*(?:not found|unavailable|not available|access denied)|unsupported model|does not exist/.test(
      text,
    )
  ) {
    return 'model-unavailable';
  }
  if (
    /network|fetch failed|connection (?:failed|reset|refused)|timed? out|timeout|dns|socket|unreachable/.test(
      text,
    )
  ) {
    return 'network';
  }
  if (
    /\b5(?:02|03|04|29)\b|overload|temporarily unavailable|server unavailable|capacity/.test(
      text,
    )
  ) {
    return 'unavailable';
  }
  if (/\b400\b|bad request|invalid request|unprocessable/.test(text)) {
    return 'invalid-request';
  }
  return 'unknown';
}

function retryPiErrorMessage(kind: PiErrorKind): string {
  switch (kind) {
    case 'rate-limit':
      return 'The model provider is receiving too many requests.';
    case 'unavailable':
      return 'The model provider is temporarily unavailable.';
    case 'network':
      return 'The model provider could not be reached.';
    default:
      return 'The reply failed.';
  }
}

export type { PiErrorDescription, PiErrorKind };

export { describePiError, retryPiErrorMessage };
