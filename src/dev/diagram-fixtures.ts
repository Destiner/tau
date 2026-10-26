const diagramCapabilities = [
  {
    type: 'flowchart',
    source:
      'flowchart LR\nA[/"First line<br/>Second &quot;quote&quot;"/] --> B[\\"Other slope<br/>A &amp; B"\\]',
    labels: ['First line', 'Second "quote"', 'Other slope', 'A & B'],
  },
  {
    type: 'sequence',
    source: 'sequenceDiagram\nTau->>Pi: prompt\nPi-->>Tau: delta',
    labels: ['Tau', 'Pi', 'prompt', 'delta'],
  },
  {
    type: 'class',
    source: 'classDiagram\nclass Runtime {\n+start()\n}\nRuntime --> Session',
    labels: ['Runtime', 'Session', 'start()'],
  },
  {
    type: 'state',
    source:
      'stateDiagram-v2\n[*] --> Idle\nIdle --> Running: prompt\nRunning --> [*]',
    labels: ['Idle', 'Running', 'prompt'],
  },
  {
    type: 'er',
    source: 'erDiagram\nSESSION ||--o{ MESSAGE : contains',
    labels: ['SESSION', 'MESSAGE', 'contains'],
  },
  {
    type: 'gitGraph',
    source:
      'gitGraph\ncommit id: "initial"\nbranch feature\ncheckout feature\ncommit id: "implementation"',
    labels: ['initial', 'implementation', 'feature'],
  },
  {
    type: 'gantt',
    source:
      'gantt\ntitle Release schedule\ndateFormat YYYY-MM-DD\ntodayMarker off\nsection Build\nImplementation :a, 2026-01-01, 2d\nVerification :after a, 1d',
    labels: ['Release schedule', 'Implementation', 'Verification'],
  },
  {
    type: 'pie',
    source: 'pie title Coverage\n"Passed" : 8\n"Pending" : 2',
    labels: ['Coverage', 'Passed', 'Pending'],
  },
  {
    type: 'mindmap',
    source: 'mindmap\n  root((Tau))\n    Runtime\n    Transcript',
    labels: ['Tau', 'Runtime', 'Transcript'],
  },
  {
    type: 'architecture',
    source:
      'architecture-beta\nservice api(server)[API]\nservice db(database)[Storage]\napi:R -- L:db',
    labels: ['API', 'Storage'],
  },
] as const;

const accessibleDiagram = `flowchart LR
accTitle: Delivery pipeline
accDescr: A request flows from queue to completion.
Queue[Queue] --> Done[Done]`;

const unsafeDiagram = `flowchart LR
A[Safe label] --> B[Destination]
click A "https://diagram.invalid/navigation"
C@{ img: "https://diagram.invalid/remote.png", label: "Remote image" }
D@{ img: "./diagram-relative.png", label: "Relative image" }
E["<img src='https://diagram.invalid/label.png' onerror='window.__TAU_MARKDOWN_IMAGE_EXECUTED__=true'>"]`;

function diagramFence(source: string): string {
  return `\`\`\`mermaid\n${source}\n\`\`\``;
}

export { accessibleDiagram, diagramCapabilities, diagramFence, unsafeDiagram };
