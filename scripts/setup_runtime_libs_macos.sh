#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RUNTIME_RELEASE_CONFIG_FILE="$SCRIPT_DIR/runtime_libs_release_config.env"

LIBS_DIR="$PROJECT_ROOT/src-tauri/libs"
VULKAN_ICD_MANIFEST_NAME="MoltenVK_icd.json"
LOCAL_BUNDLE_ARG="${1:-}"

if [[ $# -gt 1 ]]; then
  echo "[ERROR] Too many arguments."
  echo "Usage: $0 [local-mpv-bundle-dir]"
  exit 1
fi

if [[ -n "$LOCAL_BUNDLE_ARG" ]]; then
  MPV_LOCAL_BUNDLE_DIR="$LOCAL_BUNDLE_ARG"
fi

echo "📂 Project Root Directory: $PROJECT_ROOT"
mkdir -p "$LIBS_DIR"

get_runtime_release_config_value() {
  local key="$1"
  local fallback="$2"

  if [[ -f "$RUNTIME_RELEASE_CONFIG_FILE" ]]; then
    local value
    value="$(awk -v target="$key" '
      {
        line = $0
        sub(/^[[:space:]]+/, "", line)
        if (line == "" || line ~ /^#/) {
          next
        }

        pos = index(line, "=")
        if (pos == 0) {
          next
        }

        k = substr(line, 1, pos - 1)
        v = substr(line, pos + 1)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", k)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)

        if (k == target) {
          print v
          exit
        }
      }
    ' "$RUNTIME_RELEASE_CONFIG_FILE")"

    if [[ -n "$value" ]]; then
      echo "$value"
      return
    fi
  fi

  echo "$fallback"
}

is_system_dep() {
  local dep="$1"
  [[ "$dep" == /System/* || "$dep" == /usr/lib/* ]]
}

deps_of() {
  local file="$1"
  otool -L "$file" | tail -n +2 | awk '{print $1}'
}

run_install_name_tool() {
  if ! install_name_tool "$@" >/dev/null 2>&1; then
    install_name_tool "$@"
    return 1
  fi
}

add_rpath_if_missing() {
  local file="$1"
  local rpath_value="$2"
  local existing
  existing="$(otool -l "$file" | awk '/cmd LC_RPATH/{getline; getline; print $2}')"
  if ! grep -Fqx "$rpath_value" <<<"$existing"; then
    chmod u+w "$file"
    run_install_name_tool -add_rpath "$rpath_value" "$file"
  fi
}

fetch_release_json() {
  local api_url="$1"
  local auth_token="${MPV_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
  local -a curl_args

  curl_args=(
    -fsSL
    -H "Accept: application/vnd.github+json"
    -H "X-GitHub-Api-Version: 2022-11-28"
  )

  if [[ -n "$auth_token" ]]; then
    curl_args+=(-H "Authorization: Bearer $auth_token")
  fi

  if ! curl "${curl_args[@]}" "$api_url"; then
    echo "[ERROR] Failed to query release metadata: $api_url" >&2
    echo "[ERROR] If this happens in CI, set MPV_GITHUB_TOKEN or GITHUB_TOKEN to avoid anonymous GitHub API limits." >&2
    return 1
  fi
}

select_asset_url() {
  local release_json="$1"
  local explicit_url="${2:-}"
  local asset_urls
  local candidate
  local arch
  local arch_pattern
  local raw_target_arch

  if [[ -n "$explicit_url" ]]; then
    echo "$explicit_url"
    return
  fi

  asset_urls="$(printf '%s\n' "$release_json" | awk -F\" '/browser_download_url/ {print $4}')"
  if [[ -z "$asset_urls" ]]; then
    return
  fi

  raw_target_arch="${MPV_TARGET_ARCH:-}"
  if [[ -n "$raw_target_arch" ]]; then
    arch="$(printf '%s' "$raw_target_arch" | tr '[:upper:]' '[:lower:]')"
  else
    arch="$(uname -m)"
  fi

  case "$arch" in
    arm64|aarch64)
      arch='arm64'
      arch_pattern='(arm64|aarch64)'
      ;;
    x86_64|amd64|x64|intel)
      arch='x86_64'
      arch_pattern='(x86_64|amd64|x64|intel)'
      ;;
    *)
      arch_pattern="(${arch})"
      ;;
  esac

  if [[ -n "$raw_target_arch" ]]; then
    echo "🧭 Selecting macOS asset for architecture: $arch (from MPV_TARGET_ARCH=$raw_target_arch)" >&2
  else
    echo "🧭 Selecting macOS asset for architecture: $arch (from uname -m)" >&2
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(darwin|macos|osx).*(tar\.gz|tgz|zip|dylib)$' \
    | grep -Ei "$arch_pattern" \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(darwin|macos|osx).*(tar\.gz|tgz|zip|dylib)$' \
    | grep -Ei 'universal' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(darwin|macos|osx).*(tar\.gz|tgz|zip|dylib)$' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(tar\.gz|tgz|zip|dylib)$' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi
}

version_greater_than() {
  local lhs="$1"
  local rhs="$2"

  awk -v lhs="$lhs" -v rhs="$rhs" '
    function parse(v, arr, i, n) {
      n = split(v, arr, /\./)
      for (i = n + 1; i <= 4; i++) {
        arr[i] = 0
      }
    }

    BEGIN {
      parse(lhs, L)
      parse(rhs, R)
      for (i = 1; i <= 4; i++) {
        if ((L[i] + 0) > (R[i] + 0)) {
          exit 0
        }
        if ((L[i] + 0) < (R[i] + 0)) {
          exit 1
        }
      }
      exit 1
    }
  '
}

extract_dylib_minos() {
  local dylib="$1"
  local minos

  minos="$(otool -l "$dylib" | awk '
    /LC_BUILD_VERSION/ { in_build = 1; next }
    in_build && /minos/ { print $2; exit }
  ')"
  if [[ -n "$minos" ]]; then
    echo "$minos"
    return
  fi

  minos="$(otool -l "$dylib" | awk '
    /LC_VERSION_MIN_MACOSX/ { in_legacy = 1; next }
    in_legacy && /version/ { print $2; exit }
  ')"
  echo "$minos"
}

validate_dylib_minos() {
  local output_dir="$1"
  local requested_target="${MACOSX_DEPLOYMENT_TARGET:-}"
  local has_failure=0

  if [[ -z "$requested_target" ]]; then
    return
  fi

  echo "🔎 Validating dylib minOS against MACOSX_DEPLOYMENT_TARGET=$requested_target"

  while IFS= read -r dylib; do
    local minos
    minos="$(extract_dylib_minos "$dylib")"
    [[ -z "$minos" ]] && continue
    if version_greater_than "$minos" "$requested_target"; then
      echo "[ERROR] $(basename "$dylib") requires macOS $minos, but target is $requested_target."
      has_failure=1
    fi
  done < <(find "$output_dir" -maxdepth 1 -type f -name '*.dylib' | sort)

  if [[ "$has_failure" -eq 1 ]]; then
    echo "[ERROR] Runtime libs are incompatible with target macOS $requested_target."
    echo "[ERROR] Rebuild mpv libs with a lower deployment target or use a compatible release asset."
    exit 1
  fi
}

extract_asset() {
  local asset_file="$1"
  local extract_dir="$2"
  local file_name

  file_name="$(basename "$asset_file")"

  case "$file_name" in
    *.tar.gz|*.tgz)
      tar -xzf "$asset_file" -C "$extract_dir"
      ;;
    *.zip)
      unzip -q "$asset_file" -d "$extract_dir"
      ;;
    *.dylib)
      cp -f "$asset_file" "$extract_dir/"
      ;;
    *)
      echo "[ERROR] Unsupported asset format: $file_name"
      exit 1
      ;;
  esac
}

copy_dylibs_to_dir() {
  local extract_dir="$1"
  local output_dir="$2"
  local canonical_name="$3"
  local link_name="$4"
  local source_dir="$extract_dir"
  local source_real
  local output_real
  local staging_dir=""
  local dylib_count
  local root_lib

  mkdir -p "$output_dir"

  source_real="$(cd "$extract_dir" && pwd -P)"
  output_real="$(cd "$output_dir" && pwd -P)"

  # If source and destination are the same directory, stage first to avoid
  # deleting source dylibs before copying.
  if [[ "$source_real" == "$output_real" ]]; then
    staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/soia-mpv-stage.XXXXXX")"
    while IFS= read -r dylib; do
      cp -f "$dylib" "$staging_dir/$(basename "$dylib")"
    done < <(find "$extract_dir" -type f -name '*.dylib')
    source_dir="$staging_dir"
  fi

  dylib_count="$(find "$source_dir" -type f -name '*.dylib' | wc -l | tr -d ' ')"
  if [[ "$dylib_count" -eq 0 ]]; then
    echo "[ERROR] Release asset does not contain any .dylib files."
    rm -rf "$staging_dir" 2>/dev/null || true
    exit 1
  fi

  find "$output_dir" -maxdepth 1 -type f -name '*.dylib' -delete

  while IFS= read -r dylib; do
    cp -f "$dylib" "$output_dir/$(basename "$dylib")"
  done < <(find "$source_dir" -type f -name '*.dylib')

  root_lib="$output_dir/$canonical_name"
  if [[ ! -f "$root_lib" ]]; then
    root_lib="$(find "$output_dir" -maxdepth 1 -type f -name 'lib*.dylib' | head -n 1 || true)"
    if [[ -z "$root_lib" ]]; then
      echo "[ERROR] Cannot find expected dylib in source asset."
      rm -rf "$staging_dir" 2>/dev/null || true
      exit 1
    fi
    cp -f "$root_lib" "$output_dir/$canonical_name"
  fi

  if [[ ! -e "$output_dir/$link_name" ]]; then
    ln -sf "$canonical_name" "$output_dir/$link_name"
  fi

  rm -rf "$staging_dir" 2>/dev/null || true
}

copy_config_data_to_dir() {
  local source_dir="$1"
  local output_dir="$2"
  local config_source="$source_dir/config.data"

  if [[ ! -f "$config_source" ]]; then
    echo "[INFO] config.data not found in runtime bundle, skipped."
    return
  fi

  cp -f "$config_source" "$output_dir/config.data"
}

resolve_runtime_dir_from_dylibs() {
  local source_dir="$1"
  local runtime_dylib=""

  runtime_dylib="$(find "$source_dir" -type f -name '*.dylib' | head -n 1 || true)"
  if [[ -z "$runtime_dylib" ]]; then
    echo "$source_dir"
    return
  fi

  dirname "$runtime_dylib"
}

copy_vulkan_icd_manifest_to_dir() {
  local source_dir="$1"
  local output_dir="$2"
  local source_manifest=""

  source_manifest="$(find "$source_dir" -type f -name "$VULKAN_ICD_MANIFEST_NAME" | head -n 1 || true)"
  if [[ -z "$source_manifest" ]]; then
    echo "[ERROR] Cannot find $VULKAN_ICD_MANIFEST_NAME in: $source_dir"
    exit 1
  fi

  cp -f "$source_manifest" "$output_dir/$VULKAN_ICD_MANIFEST_NAME"
  echo "🧩 Installed Vulkan ICD manifest: $output_dir/$VULKAN_ICD_MANIFEST_NAME"
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

validate_vulkan_runtime_files() {
  local output_dir="$1"

  if [[ ! -f "$output_dir/libvulkan.1.dylib" ]]; then
    return
  fi

  if [[ ! -f "$output_dir/libMoltenVK.dylib" ]]; then
    echo "[ERROR] Missing libMoltenVK.dylib in runtime libs: $output_dir"
    exit 1
  fi

  if [[ ! -f "$output_dir/$VULKAN_ICD_MANIFEST_NAME" ]]; then
    echo "[ERROR] Missing $VULKAN_ICD_MANIFEST_NAME in runtime libs: $output_dir"
    exit 1
  fi
}

sync_dev_frameworks() {
  local source_dir="$1"
  local dev_frameworks_dir="$PROJECT_ROOT/src-tauri/target/Frameworks"
  local dev_icd_dir="$dev_frameworks_dir/vulkan/icd.d"
  local source_manifest="$source_dir/$VULKAN_ICD_MANIFEST_NAME"

  mkdir -p "$dev_frameworks_dir"
  find "$dev_frameworks_dir" -maxdepth 1 -type f -name '*.dylib' -delete

  while IFS= read -r dylib; do
    cp -f "$dylib" "$dev_frameworks_dir/$(basename "$dylib")"
  done < <(find "$source_dir" -maxdepth 1 -type f -name '*.dylib')

  if [[ -e "$source_dir/libmpv.dylib" ]]; then
    rm -f "$dev_frameworks_dir/libmpv.dylib"
    ln -sf "libmpv.2.dylib" "$dev_frameworks_dir/libmpv.dylib"
  fi

  mkdir -p "$dev_icd_dir"
  if [[ ! -f "$source_manifest" ]]; then
    echo "[ERROR] Missing Vulkan ICD manifest for dev runtime: $source_manifest"
    exit 1
  fi
  cp -f "$source_manifest" "$dev_icd_dir/$VULKAN_ICD_MANIFEST_NAME"
  rewrite_vulkan_icd_library_path \
    "$dev_icd_dir/$VULKAN_ICD_MANIFEST_NAME" \
    "../../../libMoltenVK.dylib"

  echo "🔄 Synced dylibs to dev frameworks: $dev_frameworks_dir"
  echo "🔄 Synced Vulkan ICD manifest to dev frameworks: $dev_icd_dir/$VULKAN_ICD_MANIFEST_NAME"
}

ad_hoc_sign_dylibs_if_macos() {
  local target_dir="$1"

  if [[ "$(uname -s)" != "Darwin" ]]; then
    return
  fi

  if ! command -v codesign >/dev/null 2>&1; then
    return
  fi

  echo "🔏 Ad-hoc signing dylibs: $target_dir"
  while IFS= read -r dylib; do
    if ! codesign --force --sign - --timestamp=none "$dylib" >/dev/null 2>&1; then
      echo "[ERROR] Failed to ad-hoc sign: $dylib"
      codesign --force --sign - --timestamp=none "$dylib"
      exit 1
    fi
  done < <(find "$target_dir" -maxdepth 1 -type f -name '*.dylib' | sort)
}

sync_tauri_macos_frameworks() {
  local sync_script="$PROJECT_ROOT/scripts/sync_runtime_libs.mjs"
  if [[ ! -f "$sync_script" ]]; then
    echo "[ERROR] Missing script: $sync_script"
    exit 1
  fi

  if ! command -v node >/dev/null 2>&1; then
    echo "[ERROR] node command not found. Cannot sync tauri macOS frameworks."
    exit 1
  fi

  node "$sync_script" --platform darwin
}

normalize_and_validate_dylibs() {
  local output_dir="$1"
  local unresolved_file
  local has_unresolved=0

  unresolved_file="$(mktemp "${TMPDIR:-/tmp}/soia-mpv-unresolved.XXXXXX")"
  trap "rm -f '$unresolved_file'" RETURN

  while IFS= read -r current; do
    [[ -z "$current" ]] && continue
    chmod u+w "$current"
    add_rpath_if_missing "$current" "@loader_path"
    run_install_name_tool -id "@rpath/$(basename "$current")" "$current"

    while IFS= read -r dep; do
      [[ -z "$dep" ]] && continue
      if is_system_dep "$dep"; then
        continue
      fi

      dep_base="$(basename "$dep")"
      dep_target="$output_dir/$dep_base"
      new_ref="@rpath/$dep_base"

      if [[ -f "$dep_target" ]]; then
        if [[ "$dep" != "$new_ref" ]]; then
          run_install_name_tool -change "$dep" "$new_ref" "$current"
        fi
      else
        has_unresolved=1
        printf '%s: %s\n' "$(basename "$current")" "$dep" >> "$unresolved_file"
      fi
    done < <(deps_of "$current")
  done < <(find "$output_dir" -maxdepth 1 -type f -name '*.dylib' | sort)

  if [[ "$has_unresolved" -eq 1 ]]; then
    echo "[ERROR] Found unresolved dylib dependencies in $output_dir:"
    sort -u "$unresolved_file" | sed 's/^/  - /'
    echo "[ERROR] Ensure MPV_LOCAL_BUNDLE_DIR includes all non-system dylibs."
    exit 1
  fi
}

install_mpv_from_local_bundle() {
  local output_dir="$1"
  local local_bundle_dir="${MPV_LOCAL_BUNDLE_DIR:-}"

  if [[ -z "$local_bundle_dir" ]]; then
    echo "[ERROR] MPV_LOCAL_BUNDLE_DIR is empty."
    exit 1
  fi
  if [[ ! -d "$local_bundle_dir" ]]; then
    echo "[ERROR] MPV_LOCAL_BUNDLE_DIR does not exist: $local_bundle_dir"
    exit 1
  fi

  echo "📦 Using local mpv bundle: $local_bundle_dir"
  local runtime_dir
  runtime_dir="$(resolve_runtime_dir_from_dylibs "$local_bundle_dir")"
  copy_dylibs_to_dir "$local_bundle_dir" "$output_dir" "libmpv.2.dylib" "libmpv.dylib"
  copy_config_data_to_dir "$runtime_dir" "$output_dir"
  copy_vulkan_icd_manifest_to_dir "$local_bundle_dir" "$output_dir"
  validate_vulkan_runtime_files "$output_dir"
  validate_dylib_minos "$output_dir"
  echo "🔧 Normalizing dylib install names for dev runtime..."
  normalize_and_validate_dylibs "$output_dir"
  sync_tauri_macos_frameworks
  sync_dev_frameworks "$output_dir"
  ad_hoc_sign_dylibs_if_macos "$output_dir"
  ad_hoc_sign_dylibs_if_macos "$PROJECT_ROOT/src-tauri/target/Frameworks"
  echo "✅ mpv local bundle installed: $output_dir"
}

download_mpv() {
  local output_dir="$LIBS_DIR/mpv"
  local default_release_repo
  local default_release_tag
  default_release_repo="$(get_runtime_release_config_value "MPV_RELEASE_REPO" "")"
  default_release_tag="$(get_runtime_release_config_value "MPV_RELEASE_TAG" "")"
  if [[ -z "$default_release_repo" || -z "$default_release_tag" ]]; then
    echo "[ERROR] Missing MPV release defaults in $RUNTIME_RELEASE_CONFIG_FILE"
    echo "[ERROR] Expected keys: MPV_RELEASE_REPO and MPV_RELEASE_TAG"
    exit 1
  fi
  local release_repo="${MPV_RELEASE_REPO:-$default_release_repo}"
  local release_tag="${MPV_RELEASE_TAG:-$default_release_tag}"
  local release_api_url="https://api.github.com/repos/${release_repo}/releases/tags/${release_tag}"
  local release_json
  local asset_url
  local tmp_dir
  local asset_name
  local asset_file
  local extract_dir

  if [[ -n "${MPV_LOCAL_BUNDLE_DIR:-}" ]]; then
    install_mpv_from_local_bundle "$output_dir"
    return
  fi

  echo "📥 Resolving mpv release asset from ${release_repo}@${release_tag}..."
  release_json="$(fetch_release_json "$release_api_url")"
  asset_url="$(select_asset_url "$release_json" "${MPV_RELEASE_ASSET_URL:-}")"

  if [[ -z "$asset_url" ]]; then
    echo "[ERROR] Cannot find a downloadable asset in release ${release_tag}."
    echo "You can override via MPV_RELEASE_ASSET_URL=<direct-download-url> pnpm setup:libs"
    exit 1
  fi

  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/soia-mpv.XXXXXX")"
  trap "rm -rf '$tmp_dir'" EXIT

  asset_name="$(basename "${asset_url%%\?*}")"
  asset_file="$tmp_dir/$asset_name"
  extract_dir="$tmp_dir/extract"

  mkdir -p "$extract_dir"

  echo "⬇️ Downloading asset: $asset_name"
  curl -fL --retry 3 --retry-delay 1 -o "$asset_file" "$asset_url"

  echo "📦 Extracting asset..."
  extract_asset "$asset_file" "$extract_dir"

  echo "🧩 Installing dylibs to $output_dir"
  local runtime_dir
  runtime_dir="$(resolve_runtime_dir_from_dylibs "$extract_dir")"
  copy_dylibs_to_dir "$extract_dir" "$output_dir" "libmpv.2.dylib" "libmpv.dylib"
  copy_config_data_to_dir "$runtime_dir" "$output_dir"
  copy_vulkan_icd_manifest_to_dir "$extract_dir" "$output_dir"
  validate_vulkan_runtime_files "$output_dir"
  validate_dylib_minos "$output_dir"
  echo "🔧 Normalizing dylib install names for dev runtime..."
  normalize_and_validate_dylibs "$output_dir"
  sync_tauri_macos_frameworks
  sync_dev_frameworks "$output_dir"
  ad_hoc_sign_dylibs_if_macos "$output_dir"
  ad_hoc_sign_dylibs_if_macos "$PROJECT_ROOT/src-tauri/target/Frameworks"
  echo "✅ mpv completed: $output_dir"
}

download_all() {
  download_mpv
}

download_all
