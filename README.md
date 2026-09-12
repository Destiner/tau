# Tau

A minimal, focused interface for [Pi](https://pi.dev).

<img width="1806" height="1169" alt="Screenshot 2026-09-09 at 21 45 44" src="https://github.com/user-attachments/assets/f6dc8915-b74c-40c3-b9e3-9c7600295f2d" />

## Features

- Project pane with chat persistance
- Remote projects via SSH
- Support for [Pi Extensions](https://pi.dev/docs/latest/extensions)
- Archived sessions
- Mermaid diagram rendering

## Roadmap

- Search
- Steering
- File operations (explorer/editor)
- Keyboard shortcuts
- Command palette

## Non goals

- Support for other harnesses
- Integrated terminal and browser

## Philosophy

Pi is good because it's lean, stable, and extensible. Instead of forcing opinions on you, it provides a solid core for the agent loop and an API surface for everything else. By saying no to a hundred things, it delivers something that everyone finds usable. For everything else, "There's an extension for that". That gives you a sense of ownership and doesn't lock you into bad decisions.

Tau builds on that idea and attempts to do the same at the UI level. Instead of shipping every feature, it focuses on the core things — sessions, models, and transcripts — and does them well. Something that you use 10 hours a day has to be fast, reliable, and provide a feeling of ownership.

Coding is changing every month now. The best coding tools are simple at the core and extensible at the edges. A lot of agent orchestration tools make a mistake of rushing to ship as many features with LLMs as physically possible. Half of those features become redundant within a month. The work is getting increasingly high level every month. Things like sophisticated code review tools, subagent visibility, and built-in VCS and project management integrations make less sense as models become more capable.

Also, no tabs.

## Inspirations

- [Zed](https://zed.dev)
- [Linear](https://linear.app)
- [fx](https://fx.sh)
