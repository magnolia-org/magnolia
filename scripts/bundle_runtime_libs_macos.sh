#!/bin/bash
set -euo pipefail

usage() {
  cat <<USAGE
Usage:
  $0 <path-to-app> [--root-lib <name>] [--strict-deps] [--allow-unresolved] [--skip-sign] [--sign-identity <identity>]

Examples:
  $0 src-tauri/target/debug/bundle/macos/Magnolia.app
  $0 src-tauri/target/release/bundle/macos/Magnolia.app --sign-identity "Developer ID Application: Your Name (TEAMID)"
USAGE
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
INSTALLED_LIBS_DIR="$PROJECT_ROOT/src-tauri/libs/mpv"
VULKAN_ICD_MANIFEST_NAME="MoltenVK_icd.json"

APP_DIR="$1"
shift

ROOT_LIB_NAME="libmpv.2.dylib"
DO_SIGN=1
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
STRICT_UNRESOLVED="auto"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root-lib)
      ROOT_LIB_NAME="${2:-}"
      shift 2
      ;;
    --strict-deps)
      STRICT_UNRESOLVED=1
      shift
      ;;
    --allow-unresolved)
      STRICT_UNRESOLVED=0
      shift
      ;;
    --skip-sign)
      DO_SIGN=0
      shift
      ;;
    --sign-identity)
      SIGN_IDENTITY="${2:-}"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1"
      usage
      exit 1
      ;;
  esac
done

if [[ ! -d "$APP_DIR" ]]; then
  echo "[ERROR] App bundle not found: $APP_DIR"
  exit 1
fi

if [[ "$STRICT_UNRESOLVED" == "auto" ]]; then
  if [[ "$APP_DIR" == *"/target/release/"* ]]; then
    STRICT_UNRESOLVED=1
  else
    STRICT_UNRESOLVED=0
  fi
fi

FRAMEWORKS_DIR="$APP_DIR/Contents/Frameworks"
RESOURCES_DIR="$APP_DIR/Contents/Resources"
VULKAN_ICD_DIR="$RESOURCES_DIR/vulkan/icd.d"
if [[ ! -d "$FRAMEWORKS_DIR" ]]; then
  echo "[ERROR] Frameworks dir not found: $FRAMEWORKS_DIR"
  exit 1
fi

if [[ ! -d "$INSTALLED_LIBS_DIR" ]]; then
  echo "[ERROR] Installed libs dir not found: $INSTALLED_LIBS_DIR"
  echo "[ERROR] Run: npm run setup:libs"
  exit 1
fi

echo "[INFO] Installed libs dir: $INSTALLED_LIBS_DIR"
echo "[INFO] Strict unresolved deps: $STRICT_UNRESOLVED"

ROOT_LIB_PATH="$FRAMEWORKS_DIR/$ROOT_LIB_NAME"
if [[ ! -f "$ROOT_LIB_PATH" ]]; then
  CANDIDATE="$(find "$FRAMEWORKS_DIR" -maxdepth 1 -type f -name 'libmpv*.dylib' | head -n 1 || true)"
  if [[ -n "$CANDIDATE" ]]; then
    ROOT_LIB_PATH="$CANDIDATE"
  else
    echo "[ERROR] Cannot find root mpv dylib under: $FRAMEWORKS_DIR"
    exit 1
  fi
fi

is_system_dep() {
  local dep="$1"
  [[ "$dep" == /System/* || "$dep" == /usr/lib/* ]]
}

deps_of() {
  local file="$1"
  otool -L "$file" | tail -n +2 | awk '{print $1}'
}

rewrite_vulkan_icd_library_path() {
  local manifest_path="$1"
  local library_path="$2"
  local tmp_manifest
  local api_version

  if [[ ! -f "$manifest_path" ]]; then
    echo "[ERROR] Vulkan ICD manifest not found: $manifest_path"
    exit 1
  fi

  api_version="$(awk -F\" '/"api_version"/ { print $4; exit }' "$manifest_path")"
  if [[ -z "$api_version" ]]; then
    api_version="1.0.0"
  fi

  tmp_manifest="$(mktemp "${TMPDIR:-/tmp}/soia-moltenvk-icd.XXXXXX")"
  cat > "$tmp_manifest" <<EOF
{
    "file_format_version": "1.0.0",
    "ICD": {
        "library_path": "$library_path",
        "api_version": "$api_version",
        "is_portability_driver": true
    }
}
EOF

  cp -f "$tmp_manifest" "$manifest_path"
  rm -f "$tmp_manifest"
}

contains() {
  local needle="$1"
  shift
  local item
  for item in "$@"; do
    if [[ "$item" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

add_rpath_if_missing() {
  local file="$1"
  local existing
  existing="$(otool -l "$file" | awk '/cmd LC_RPATH/{getline; getline; print $2}')"
  if ! grep -qx "@loader_path" <<<"$existing"; then
    chmod u+w "$file" 2>/dev/null || true
    install_name_tool -add_rpath "@loader_path" "$file" 2>/dev/null || true
  fi
}

resolve_dep_source() {
  local loader="$1"
  local dep="$2"
  local base
  local rel
  local candidate
  local rpath
  local resolved_rpath

  if is_system_dep "$dep"; then
    return 1
  fi

  base="$(basename "$dep")"

  # Prefer preinstalled runtime libs from npm run setup:libs.
  candidate="$INSTALLED_LIBS_DIR/$base"
  if [[ -f "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi

  # Already copied into app bundle frameworks.
  candidate="$FRAMEWORKS_DIR/$base"
  if [[ -f "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi

  if [[ "$dep" == @rpath/* || "$dep" == @loader_path/* || "$dep" == @executable_path/* ]]; then
    if [[ "$dep" == @rpath/* ]]; then
      rel="${dep#@rpath/}"
      while IFS= read -r rpath; do
        [[ -z "$rpath" ]] && continue
        case "$rpath" in
          @loader_path)
            resolved_rpath="$(dirname "$loader")"
            ;;
          @loader_path/*)
            resolved_rpath="$(dirname "$loader")/${rpath#@loader_path/}"
            ;;
          @executable_path)
            resolved_rpath="$APP_DIR/Contents/MacOS"
            ;;
          @executable_path/*)
            resolved_rpath="$APP_DIR/Contents/MacOS/${rpath#@executable_path/}"
            ;;
          *)
            resolved_rpath="$rpath"
            ;;
        esac

        candidate="$resolved_rpath/$rel"
        if [[ -f "$candidate" ]]; then
          echo "$candidate"
          return 0
        fi
      done < <(otool -l "$loader" | awk '/cmd LC_RPATH/{getline; getline; print $2}')
    fi

    if [[ "$dep" == @loader_path/* ]]; then
      rel="${dep#@loader_path/}"
      candidate="$(dirname "$loader")/$rel"
      if [[ -f "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    fi

    if [[ "$dep" == @executable_path/* ]]; then
      rel="${dep#@executable_path/}"
      candidate="$APP_DIR/Contents/MacOS/$rel"
      if [[ -f "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    fi

    return 1
  fi

  if [[ -f "$dep" ]]; then
    echo "$dep"
    return 0
  fi

  return 1
}

queue=("$ROOT_LIB_PATH")
processed=()

echo "[INFO] Root dylib: $ROOT_LIB_PATH"

while [[ ${#queue[@]} -gt 0 ]]; do
  current="${queue[0]}"
  queue=("${queue[@]:1}")

  if [[ ${#processed[@]} -gt 0 ]] && contains "$current" "${processed[@]}"; then
    continue
  fi
  processed+=("$current")

  add_rpath_if_missing "$current"
  chmod u+w "$current" 2>/dev/null || true

  while IFS= read -r dep; do
    [[ -z "$dep" ]] && continue
    if is_system_dep "$dep"; then
      continue
    fi

    dep_base="$(basename "$dep")"
    new_ref="@rpath/$dep_base"

    if [[ "$dep" != "$new_ref" ]]; then
      install_name_tool -change "$dep" "$new_ref" "$current"
    fi

    if src_dep="$(resolve_dep_source "$current" "$dep")"; then
      target_dep="$FRAMEWORKS_DIR/$dep_base"
      if [[ ! -f "$target_dep" ]]; then
        cp -fL "$src_dep" "$target_dep"
        chmod u+w "$target_dep" 2>/dev/null || true
        install_name_tool -id "@rpath/$dep_base" "$target_dep" 2>/dev/null || true
        echo "[COPY] $dep_base"
      fi
      queue+=("$target_dep")
    else
      if [[ "$STRICT_UNRESOLVED" -eq 1 ]]; then
        echo "[ERROR] Unresolved dependency referenced by $(basename "$current"): $dep"
        exit 1
      else
        echo "[WARN] Unresolved dependency referenced by $(basename "$current"): $dep"
      fi
    fi
  done < <(deps_of "$current")
done

if [[ -f "$FRAMEWORKS_DIR/libvulkan.1.dylib" || -f "$FRAMEWORKS_DIR/libvulkan.dylib" ]]; then
  if [[ ! -f "$FRAMEWORKS_DIR/libMoltenVK.dylib" ]]; then
    if [[ "$STRICT_UNRESOLVED" -eq 1 ]]; then
      echo "[ERROR] Missing libMoltenVK.dylib in app frameworks while Vulkan loader is present."
      exit 1
    else
      echo "[WARN] Missing libMoltenVK.dylib in app frameworks while Vulkan loader is present."
    fi
  fi

  icd_manifest_source="$INSTALLED_LIBS_DIR/$VULKAN_ICD_MANIFEST_NAME"
  if [[ ! -f "$icd_manifest_source" ]]; then
    if [[ "$STRICT_UNRESOLVED" -eq 1 ]]; then
      echo "[ERROR] Missing Vulkan ICD manifest: $icd_manifest_source"
      exit 1
    else
      echo "[WARN] Missing Vulkan ICD manifest: $icd_manifest_source"
    fi
  else
    mkdir -p "$VULKAN_ICD_DIR"
    cp -f "$icd_manifest_source" "$VULKAN_ICD_DIR/$VULKAN_ICD_MANIFEST_NAME"
    rewrite_vulkan_icd_library_path \
      "$VULKAN_ICD_DIR/$VULKAN_ICD_MANIFEST_NAME" \
      "../../../Frameworks/libMoltenVK.dylib"
    echo "[COPY] vulkan/icd.d/$VULKAN_ICD_MANIFEST_NAME"
  fi
fi

if [[ $DO_SIGN -eq 1 ]]; then
  echo "[INFO] Ad-hoc signing dylibs and app bundle..."
  while IFS= read -r dylib; do
    codesign --force --sign "$SIGN_IDENTITY" --timestamp=none "$dylib"
  done < <(find "$FRAMEWORKS_DIR" -maxdepth 1 -type f -name '*.dylib' | sort)
  codesign --force --deep --sign "$SIGN_IDENTITY" --timestamp=none "$APP_DIR"
fi

echo "[DONE] Bundled non-system dylibs into: $FRAMEWORKS_DIR"
