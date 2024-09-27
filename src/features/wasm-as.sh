for FEATURE in $(ls . | grep ".wat" | sed 's/\.[^.]*$//')
do
    echo "=== ${FEATURE} ==="

    wasm-as --mvp-features $(head -1 "${FEATURE}.wat" | cut -c 13-) "${FEATURE}.wat" || exit 1
    wasm-dis "${FEATURE}.wasm" || exit 1
    
    echo "=== ${FEATURE} ==="
done
