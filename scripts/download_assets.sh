#!/usr/bin/env bash
set -euo pipefail

# Rebuild the local asset library from upstream sources.
#
# This script intentionally performs network operations. The OpenCode agent only
# writes this script; a human operator runs it when they are ready to download the
# third-party assets and review their licenses.

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ASSETS_DIR="${ROOT_DIR}/assets"
MODEL_DIR="${ASSETS_DIR}/models"
TEXTURE_DIR="${ASSETS_DIR}/textures"
DOWNLOAD_DIR="${RIG_ASSET_DOWNLOAD_CACHE:-${ROOT_DIR}/.asset-downloads}"

COMMON_MODELS_URL="https://github.com/alecjacobson/common-3d-test-models"
KEENAN_BASE_URL="https://www.cs.cmu.edu/~kmcrane/Projects/ModelRepository"

require_command() {
    local command_name="$1"
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        echo "error: required command not found: ${command_name}" >&2
        exit 1
    fi
}

download_file() {
    local url="$1"
    local output="$2"

    mkdir -p "$(dirname -- "${output}")"
    if [[ -s "${output}" ]]; then
        echo "skip existing download: ${output}"
        return
    fi

    local temp_output="${output}.tmp"
    rm -f "${temp_output}"

    echo "download: ${url}"
    if command -v curl >/dev/null 2>&1; then
        curl --fail --location --output "${temp_output}" "${url}"
    elif command -v wget >/dev/null 2>&1; then
        wget --output-document="${temp_output}" "${url}"
    else
        echo "error: curl or wget is required for downloads" >&2
        exit 1
    fi

    mv "${temp_output}" "${output}"
}

copy_if_missing() {
    local source="$1"
    local destination="$2"

    mkdir -p "$(dirname -- "${destination}")"
    if [[ -s "${destination}" ]]; then
        echo "skip existing asset: ${destination}"
        return
    fi

    cp "${source}" "${destination}"
    echo "copied: ${destination}"
}

first_match() {
    local root="$1"
    local pattern="$2"

    find "${root}" \
        -type f \
        ! -path '*/__MACOSX/*' \
        -iname "${pattern}" \
        -print \
        -quit
}

parse_reference() {
    local keyword="$1"
    local file="$2"

    awk -v key="${keyword}" '
        /^[[:space:]]*#/ { next }
        tolower($1) == key { print $NF; exit }
    ' "${file}"
}

normalise_reference() {
    printf '%s\n' "$1" | tr '\\' '/'
}

resolve_reference() {
    local base_dir="$1"
    local search_root="$2"
    local reference
    reference="$(normalise_reference "$3")"

    if [[ -f "${base_dir}/${reference}" ]]; then
        printf '%s\n' "${base_dir}/${reference}"
        return
    fi

    local reference_name
    reference_name="$(basename -- "${reference}")"
    first_match "${search_root}" "${reference_name}"
}

rewrite_reference_line() {
    local keyword="$1"
    local replacement="$2"
    local source="$3"
    local destination="$4"

    awk -v key="${keyword}" -v replacement="${replacement}" '
        tolower($1) == tolower(key) && replaced == 0 {
            print key " " replacement
            replaced = 1
            next
        }
        { print }
    ' "${source}" >"${destination}"
}

prepare_directories() {
    mkdir -p \
        "${DOWNLOAD_DIR}" \
        "${MODEL_DIR}/spot" \
        "${MODEL_DIR}/ogre" \
        "${MODEL_DIR}/bob" \
        "${MODEL_DIR}/blub" \
        "${TEXTURE_DIR}/brick_red" \
        "${TEXTURE_DIR}/wood_oak" \
        "${TEXTURE_DIR}/metal_rust" \
        "${TEXTURE_DIR}/stone_cobble" \
        "${TEXTURE_DIR}/marble_white" \
        "${TEXTURE_DIR}/concrete_worn" \
        "${TEXTURE_DIR}/fabric_denim" \
        "${TEXTURE_DIR}/terrain_grass"
}

download_geometry_models() {
    local checkout_dir="${DOWNLOAD_DIR}/common-3d-test-models"

    if [[ -d "${checkout_dir}/.git" ]]; then
        echo "skip existing checkout: ${checkout_dir}"
    else
        git clone --depth 1 "${COMMON_MODELS_URL}" "${checkout_dir}"
    fi

    copy_if_missing "${checkout_dir}/data/teapot.obj" "${MODEL_DIR}/teapot.obj"
    copy_if_missing "${checkout_dir}/data/stanford-bunny.obj" "${MODEL_DIR}/bunny.obj"
    copy_if_missing "${checkout_dir}/data/happy.obj" "${MODEL_DIR}/buddha.obj"
    copy_if_missing "${checkout_dir}/data/xyzrgb_dragon.obj" "${MODEL_DIR}/dragon.obj"
    copy_if_missing "${checkout_dir}/data/armadillo.obj" "${MODEL_DIR}/armadillo.obj"
    copy_if_missing "${checkout_dir}/data/suzanne.obj" "${MODEL_DIR}/suzanne.obj"
    copy_if_missing "${checkout_dir}/data/nefertiti.obj" "${MODEL_DIR}/nefertiti.obj"
}

print_extracted_listing() {
    local label="$1"
    local extract_dir="$2"

    echo
    echo "--- Extracted contents for ${label} ---"
    find "${extract_dir}" -print | sort
    echo "--- End contents for ${label} ---"
    echo
}

obj_candidate() {
    local extract_dir="$1"
    local candidate

    candidate="$(first_match "${extract_dir}" '*tri*.obj')"
    if [[ -n "${candidate}" ]]; then
        printf '%s\n' "${candidate}"
        return
    fi

    first_match "${extract_dir}" '*.obj'
}

flatten_keenan_model() {
    local name="$1"
    local extract_dir="$2"
    local destination_dir="${MODEL_DIR}/${name}"
    local destination_obj="${destination_dir}/${name}.obj"

    mkdir -p "${destination_dir}"

    local source_obj
    source_obj="$(obj_candidate "${extract_dir}")"
    if [[ -z "${source_obj}" ]]; then
        echo "warning: no OBJ found for ${name}; inspect ${extract_dir}" >&2
        return
    fi

    local mtl_ref
    mtl_ref="$(parse_reference mtllib "${source_obj}")"
    if [[ -z "${mtl_ref}" ]]; then
        echo "warning: ${source_obj} has no mtllib line; inspect manually" >&2
        return
    fi

    local source_mtl
    source_mtl="$(resolve_reference "$(dirname -- "${source_obj}")" "${extract_dir}" "${mtl_ref}")"
    if [[ -z "${source_mtl}" ]]; then
        echo "warning: could not resolve MTL '${mtl_ref}' for ${source_obj}" >&2
        return
    fi

    local texture_ref
    texture_ref="$(parse_reference map_kd "${source_mtl}")"
    if [[ -z "${texture_ref}" ]]; then
        echo "warning: ${source_mtl} has no map_Kd line; inspect manually" >&2
        return
    fi

    local source_texture
    source_texture="$(resolve_reference "$(dirname -- "${source_mtl}")" "${extract_dir}" "${texture_ref}")"
    if [[ -z "${source_texture}" ]]; then
        echo "warning: could not resolve diffuse texture '${texture_ref}' for ${source_mtl}" >&2
        return
    fi

    local destination_mtl="${destination_dir}/$(basename -- "${source_mtl}")"
    local destination_texture="${destination_dir}/$(basename -- "${source_texture}")"

    if [[ -s "${destination_obj}" && -s "${destination_mtl}" && -s "${destination_texture}" ]]; then
        echo "skip existing flattened model: ${destination_dir}"
        return
    fi

    rewrite_reference_line mtllib "$(basename -- "${destination_mtl}")" "${source_obj}" "${destination_obj}"
    rewrite_reference_line map_Kd "$(basename -- "${destination_texture}")" "${source_mtl}" "${destination_mtl}"
    cp "${source_texture}" "${destination_texture}"

    local verified_mtl
    verified_mtl="$(parse_reference mtllib "${destination_obj}")"
    local verified_texture
    verified_texture="$(parse_reference map_kd "${destination_mtl}")"

    if [[ ! -f "${destination_dir}/${verified_mtl}" || ! -f "${destination_dir}/${verified_texture}" ]]; then
        echo "error: flattened ${name} did not produce a sibling OBJ/MTL/texture chain" >&2
        exit 1
    fi

    echo "flattened ${name}: ${destination_obj}"
    echo "  mtllib -> ${verified_mtl}"
    echo "  map_Kd -> ${verified_texture}"
}

download_keenan_model() {
    local name="$1"
    local url="${KEENAN_BASE_URL}/${name}.zip"
    local zip_path="${DOWNLOAD_DIR}/keenan/${name}.zip"
    local extract_dir="${DOWNLOAD_DIR}/keenan/${name}"

    download_file "${url}" "${zip_path}"

    if [[ -f "${extract_dir}/.extracted" ]]; then
        echo "skip existing extraction: ${extract_dir}"
    else
        rm -rf "${extract_dir}"
        mkdir -p "${extract_dir}"
        unzip -q "${zip_path}" -d "${extract_dir}"
        touch "${extract_dir}/.extracted"
    fi

    print_extracted_listing "${name}" "${extract_dir}"
    flatten_keenan_model "${name}" "${extract_dir}"
}

copy_ambient_map() {
    local extract_dir="$1"
    local pattern="$2"
    local destination="$3"
    local source

    if [[ -s "${destination}" ]]; then
        echo "skip existing texture: ${destination}"
        return
    fi

    source="$(first_match "${extract_dir}" "${pattern}")"
    if [[ -z "${source}" ]]; then
        echo "error: could not find '${pattern}' in ${extract_dir}" >&2
        exit 1
    fi

    cp "${source}" "${destination}"
    echo "copied texture: ${destination}"
}

download_ambient_texture_set() {
    local ambient_id="$1"
    local destination_name="$2"
    local destination_dir="${TEXTURE_DIR}/${destination_name}"
    local zip_path="${DOWNLOAD_DIR}/ambientcg/${ambient_id}_2K-PNG.zip"
    local extract_dir="${DOWNLOAD_DIR}/ambientcg/${ambient_id}"

    mkdir -p "${destination_dir}"
    download_file "https://ambientcg.com/get?file=${ambient_id}_2K-PNG.zip" "${zip_path}"

    if [[ -f "${extract_dir}/.extracted" ]]; then
        echo "skip existing extraction: ${extract_dir}"
    else
        rm -rf "${extract_dir}"
        mkdir -p "${extract_dir}"
        unzip -q "${zip_path}" -d "${extract_dir}"
        touch "${extract_dir}/.extracted"
    fi

    copy_ambient_map "${extract_dir}" '*_Color.png' "${destination_dir}/diffuse.png"
    copy_ambient_map "${extract_dir}" '*_NormalGL.png' "${destination_dir}/normal.png"
    copy_ambient_map "${extract_dir}" '*_Roughness.png' "${destination_dir}/roughness.png"
    copy_ambient_map "${extract_dir}" '*_AmbientOcclusion.png' "${destination_dir}/ao.png"
}

main() {
    require_command git
    require_command unzip
    require_command find

    prepare_directories

    download_geometry_models


    download_keenan_model spot
    download_keenan_model ogre
    download_keenan_model bob
    download_keenan_model blub

    download_ambient_texture_set Bricks076 brick_red
    download_ambient_texture_set WoodFloor051 wood_oak
    download_ambient_texture_set Metal032 metal_rust
    download_ambient_texture_set PavingStones131 stone_cobble
    download_ambient_texture_set Marble012 marble_white
    download_ambient_texture_set Concrete034 concrete_worn
    download_ambient_texture_set Fabric045 fabric_denim
    download_ambient_texture_set Ground037 terrain_grass

    echo
    echo "Asset download pass complete."
    echo "Review any warning lines above before committing downloaded assets."
    echo "The Keenan Crane OBJ/MTL/texture chains must be verified before model_gallery paths are finalized."
}

main "$@"
