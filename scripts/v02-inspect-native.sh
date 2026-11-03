#!/bin/sh
# Build-time inspection only; no runtime/install dependency on these tools.
set -eu
export LC_ALL=C
[ "$#" -eq 1 ] || exit 2
root=$1
case $(uname -s) in
  Linux)
    case $(uname -m) in
      x86_64) loader=ld-linux-x86-64.so.2 ;;
      aarch64) loader=ld-linux-aarch64.so.1 ;;
      *) exit 2 ;;
    esac
    for binary in marmish marmishd marmish-worker; do
      file=$root/$binary
      needed=$(readelf -d "$file" | awk '/\(NEEDED\)/ { gsub(/^.*\[/, ""); gsub(/\].*$/, ""); print }')
      [ -n "$needed" ] || { echo 'native inspection: missing ELF dependencies' >&2; exit 1; }
      printf '%s\n' "$needed" | while IFS= read -r library; do
        case $library in
          libc.so.6|libm.so.6|libgcc_s.so.1|libpthread.so.0|libdl.so.2|"$loader") ;;
          *) echo 'native inspection: unqualified ELF dependency' >&2; exit 1 ;;
        esac
      done
      highest=$(readelf -VW "$file" | awk '
        {for(i=1;i<NF;i++) if($i=="Name:" && $(i+1) ~ /^GLIBC_/) {
          version=$(i+1); sub(/^GLIBC_/, "", version)
          if(version !~ /^[0-9]+[.][0-9]+([.][0-9]+)?$/) {bad=1; next}
          split(version,v,".")
          if(v[1]>2 || (v[1]==2 && v[2]>39)) {bad=1; next}
          if(!seen || v[1]>major || (v[1]==major && v[2]>minor) || (v[1]==major && v[2]==minor && v[3]>patch)) {
            highest=version; major=v[1]; minor=v[2]; patch=v[3]
          }
          seen=1
        }}
        END {if(bad || !seen) exit 1; print highest}')
      printf 'native inspection: %s highest GLIBC_%s (qualified baseline 2.39)\n' "$binary" "$highest"
    done
    ;;
  Darwin)
    for binary in marmish marmishd marmish-worker; do
      file=$root/$binary
      minimum=$(otool -l "$file" | awk '$1=="cmd" {build=($2=="LC_BUILD_VERSION")} build && $1=="minos" {print $2}')
      [ "$minimum" = 15.0 ] || { printf 'native inspection: %s unexpected Mach-O minimum\n' "$binary" >&2; exit 1; }
      dependencies=$(otool -L "$file" | awk 'NR>1 {print $1}')
      printf '%s\n' "$dependencies" | awk '$0=="/usr/lib/libSystem.B.dylib" {found=1} END {exit !found}' || { printf 'native inspection: %s missing Mach-O libSystem dependency\n' "$binary" >&2; exit 1; }
      printf '%s\n' "$dependencies" | while IFS= read -r library; do
        case $library in
          /usr/lib/libSystem.B.dylib|/usr/lib/libiconv.2.dylib|/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation|/System/Library/Frameworks/Security.framework/Versions/A/Security|/System/Library/Frameworks/SystemConfiguration.framework/Versions/A/SystemConfiguration|/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit) ;;
          *) display=$(printf '%s' "$library" | tr -c 'A-Za-z0-9_./@+-' '?' | cut -c 1-240)
             printf 'native inspection: %s unqualified Mach-O dependency: %s\n' "$binary" "$display" >&2
             exit 1 ;;
        esac
      done
      printf 'native inspection: %s macOS minimum 15.0 and qualified dependencies\n' "$binary"
    done
    ;;
  *) exit 2 ;;
esac
