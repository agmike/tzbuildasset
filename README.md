# Trainz Asset Builder.
```
Usage:
  tzbuildasset build   [options] [INPUT]
  tzbuildasset install [options] [INPUT]

Options:
  -r --recursive       Search for assets in all subfolders recursively
  -c --config          Show path to config.txt in output
  -k --kuid            Show KUID in output
  --trainzutil PATH    Path to TrainzUtil executable
  -v --verbose         Detailed output
  -s --silent          Silent output
  --temp-dir PATH      Use specified temporary directory
  -h --help            Show help
  --version            Show version

Builds all assets within given path with TrainzUtil.
Commands:
  build                Builds by installing temporary asset with dummy KUID.
  install              Installs asset directly.

Assets are determined by searching for `config.txt` file which contains string like:
kuid <(kuid|kuid2):[0-9]+:[0-9]+:[0-9]+>
```

## Usage with [atom-build](https://atom.io/packages/atom-build)
Put following in your `.atom-build.json`:
```json
{
  "cmd": "D:\\TrainzDev\\tzbuildasset\\target\\release\\tzbuildasset.exe",
  "sh": false,
  "name": "build",
  "args": [ "build", "src", "-r", "--temp-dir", "out\\build" ],
  "cwd": "{PROJECT_PATH}",
  "env": {
    "TRAINZUTIL_PATH": "D:\\TS12\\bin\\TrainzUtil.exe"
  },
  "targets": {
    "install": {
      "cmd": "D:\\TrainzDev\\tzbuildasset\\target\\release\\tzbuildasset.exe",
      "sh": false,
      "name": "install",
      "args": [ "install", "src", "-r" ],
      "cwd": "{PROJECT_PATH}",
      "env": {
        "TRAINZUTIL_PATH": "D:\\TS12\\bin\\TrainzUtil.exe"
      }
    }
  }
}
```
