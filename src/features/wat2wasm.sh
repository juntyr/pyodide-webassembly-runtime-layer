for FEATURE in $(ls . | grep ".wat" | sed 's/\.[^.]*$//')
do
    echo "=== ${FEATURE} ==="
    if [[ $(wat2wasm --help | grep -- "--enable-${FEATURE}" | wc -c) -eq 0 ]]; then
        wat2wasm "${FEATURE}.wat"
        wasm2wat "${FEATURE}.wasm"
    else
        wat2wasm "${FEATURE}.wat" "--enable-${FEATURE}"
        wasm2wat "${FEATURE}.wasm" "--enable-${FEATURE}"
    fi
    echo "=== ${FEATURE} ==="
done
