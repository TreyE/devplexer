# Devplexer

A tool for starting multiple developer commands in your projects - with seperate window support for each command.

Horribly abuses tmux to multiplex your services.

Currently only supports iTerm.

Used by creating a devplexer.yml file in your working directory, here's an example:
```yaml
namespace: localstack-viewer
apps:
  localstack:
    command: localstack start
  server:
    working_directory: server
    command: LOCALSTACK_URL=http://localhost:4566 cargo run
  ui-tailwind:
    working_directory: ui
    command: npx @tailwindcss/cli -i ./input.css -o ./assets/tailwind.css --watch
  ui:
    working_directory: ui
    command: dx serve --port 8080
```
