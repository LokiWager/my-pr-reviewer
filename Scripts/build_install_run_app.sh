#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_YML="$ROOT_DIR/project.yml"
PROJECT_PATH="$ROOT_DIR/PRReviewer.xcodeproj"
SCHEME="PRReviewerApp"
CONFIGURATION="Debug"
DESTINATION="platform=macOS"
DERIVED_DATA_PATH="$ROOT_DIR/build/DerivedData"
INSTALL_DIR="$HOME/Applications"
INSTALL_APP=0
RUN_APP=0

usage() {
  cat <<USAGE
Usage: $(basename "$0") [options]

Build/install/run PRReviewer macOS app.

Options:
  --configuration <Debug|Release>   Build configuration (default: Debug)
  --derived-data <path>             DerivedData output path (default: $ROOT_DIR/build/DerivedData)
  --install                          Install app to ~/Applications/PRReviewerApp.app
  --run                              Launch app after build (or after install when --install is set)
  --help                             Show this help

Examples:
  $(basename "$0")
  $(basename "$0") --configuration Release --install
  $(basename "$0") --configuration Release --install --run
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --configuration)
      CONFIGURATION="${2:-}"
      shift 2
      ;;
    --derived-data)
      DERIVED_DATA_PATH="${2:-}"
      shift 2
      ;;
    --install)
      INSTALL_APP=1
      shift
      ;;
    --run)
      RUN_APP=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac

done

if [[ "$CONFIGURATION" != "Debug" && "$CONFIGURATION" != "Release" ]]; then
  echo "Invalid --configuration: $CONFIGURATION (must be Debug or Release)" >&2
  exit 1
fi

if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "xcodebuild not found. Please install Xcode command line tools." >&2
  exit 1
fi

needs_generate=0
if [[ ! -d "$PROJECT_PATH" ]]; then
  needs_generate=1
elif [[ -f "$PROJECT_YML" && "$PROJECT_YML" -nt "$PROJECT_PATH" ]]; then
  needs_generate=1
fi

if [[ "$needs_generate" -eq 1 ]]; then
  if ! command -v xcodegen >/dev/null 2>&1; then
    echo "xcodegen not found but project generation is required." >&2
    echo "Install xcodegen and run: cd $ROOT_DIR && xcodegen generate" >&2
    exit 1
  fi
  echo "Generating Xcode project with xcodegen..."
  (cd "$ROOT_DIR" && xcodegen generate)
fi

echo "Building $SCHEME ($CONFIGURATION)..."
xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme "$SCHEME" \
  -configuration "$CONFIGURATION" \
  -destination "$DESTINATION" \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  build

APP_PRODUCT_PATH="$DERIVED_DATA_PATH/Build/Products/$CONFIGURATION/PRReviewerApp.app"
if [[ ! -d "$APP_PRODUCT_PATH" ]]; then
  echo "Build finished but app not found: $APP_PRODUCT_PATH" >&2
  exit 1
fi

echo "Build succeeded: $APP_PRODUCT_PATH"

if [[ "$INSTALL_APP" -eq 1 ]]; then
  TARGET_APP_PATH="$INSTALL_DIR/PRReviewerApp.app"
  mkdir -p "$INSTALL_DIR"
  rm -rf "$TARGET_APP_PATH"
  cp -R "$APP_PRODUCT_PATH" "$TARGET_APP_PATH"
  echo "Installed app: $TARGET_APP_PATH"
fi

if [[ "$RUN_APP" -eq 1 ]]; then
  if [[ "$INSTALL_APP" -eq 1 ]]; then
    open "$INSTALL_DIR/PRReviewerApp.app"
    echo "Launched: $INSTALL_DIR/PRReviewerApp.app"
  else
    open "$APP_PRODUCT_PATH"
    echo "Launched: $APP_PRODUCT_PATH"
  fi
fi
