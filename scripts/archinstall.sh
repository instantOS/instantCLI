#!/bin/sh

set -eu

ONLY_ANIMATION=0

usage() {
	cat <<EOF
Usage: archinstall.sh [OPTIONS]

Options:
  --only-animation, --animation-only  Play the logo animation and exit
  -h, --help                          Show this help message
EOF
}

parse_args() {
	while [ $# -gt 0 ]; do
		case "$1" in
		--only-animation | --animation-only)
			ONLY_ANIMATION=1
			;;
		-h | --help)
			usage
			exit 0
			;;
		*)
			echo "Unknown option: $1" >&2
			usage >&2
			exit 1
			;;
		esac
		shift
	done
}

f1() {
	cat <<'FRM'
       ▄▄▄▄               
   ▄▄████████▄            
  ▄████▀▀▀▀████▄          
  ████     ▄████          
  ▀████▄▄▄██████          
    ▀▀██████████▄▄▄▄      
               ██████▄    
               ███████    
                ▀▀▀▀▀     
                          
FRM
}

f2() {
	cat <<'FRM'
       ▄▄▄▄               
   ▄▄████████▄            
  ▄████▀▀▀▀████▄          
  ████      ████          
  ▀████▄▄▄██████          
    ▀▀██████████▄▄▄▄      
               ██████▄    
               ███████    
                ▀▀▀▀▀     
                          
FRM
}

f3() {
	cat <<'FRM'
       ▄▄▄▄               
    ▄█████████▄           
  ▄████▀▀▀▀▀███▄          
  ████▄     ████          
   █████▄▄██████          
    ▀▀██████████▄▄▄       
               ██████▄    
               ███████    
                ▀▀▀▀▀     
                          
FRM
}

f4() {
	cat <<'FRM'
       ▄▄▄▄▄              
    ▄█████████▄           
   ████▀▀  ▀████          
   ████     ████          
   ▀████████████          
     ▀▀▀████████▄         
               ██████▄    
               ███████    
                ▀▀█▀▀     
                          
FRM
}

f5() {
	cat <<'FRM'
       ▄▄▄▄▄▄             
    ▄██████████▄          
   ▄████▀   ▀████         
   ▀███▄    ▄████         
    ▀███████████          
      ▀▀▀▀██████          
               ██████▄    
              ████████    
               ▀████▀     
                          
FRM
}

f6() {
	cat <<'FRM'
       ▄▄▄▄▄▄▄            
     ▄██████████▄         
    ████▀    ████         
    ████▄   ▄████         
     ▀███████████         
        ▀▀▀▀████          
               ▄█████▄     
               ████████    
               ▀▀████▀     
                          
FRM
}

f7() {
	cat <<'FRM'
        ▄▄▄▄▄▄▄           
      ▄██████████▄        
     ████▀    ████        
     ▀███▄▄  ▄████        
      ▀██████████         
         ▀▀▀▀███          
               ▄████▄▄     
               ███████▄    
               ▀█████▀     
                  ▀        
FRM
}

f8() {
	cat <<'FRM'
         ▄▄▄██▄▄          
        ▄██████████        
       ████     ████       
       ▀███▄▄▄▄▄████       
        ▀▀████████▀        
            ▀▀▀██▀         
               ████▄▄      
              ████████     
              ▀██████▀     
                ▀▀▀▀       
FRM
}

f9() {
	cat <<'FRM'
           ▄▄▄█▄▄▄         
         ▄██████████       
        ▄███▀    ████      
         ████▄▄▄▄███▀      
          ▀████████▀       
             ▀▀██▀         
              ▄████▄       
             ▄███████▄     
             ▀███████▀     
               ▀▀▀▀▀       
FRM
}

f10() {
	cat <<'FRM'
           ▄▄▄▄▄▄▄        
         ▄██████████      
         ███▀    ████     
         ████▄▄▄▄███▀     
          ▀████████▀      
             ▀██▀         
             ▄███▄▄       
           ▄████████      
           ▀████████      
             ▀███▀▀       
FRM
}

f11() {
	cat <<'FRM'
             ▄▄▄▄▄▄       
           ▄████████▄     
          ████    ████    
          ████▄▄▄▄████    
            ▀████████▀     
              ▀██▀▀        
             ▄▄███▄        
           ▄████████       
           ▀████████       
            ▀▀████▀        
FRM
}

f12() {
	cat <<'FRM'
                ▄▄        
            ▄████████▄    
           ▄███▀▀▀▀████   
           ▀███▄▄▄▄████   
            ▀████████▀    
              ███▀▀       
           ▄▄███▄         
         ▄████████▄       
         ▀████████▀       
          ▀██████▀        
FRM
}

f13() {
	cat <<'FRM'
                          
              ▄██████▄    
             ████▀▀████▄  
            ▀████▄ ▄████  
             ▀████████▀   
              ███▀▀▀      
           ▄▄████▄         
         ▄████████▄        
         ██████████        
          ▀██████▀         
FRM
}

f14() {
	cat <<'FRM'
                          
                ▄▄▄▄▄▄    
              ▄█████████  
              ████  █████ 
              ██████████  
              ▄████▀▀▀    
        ▄▄██████          
       ██████████         
       ██████████         
        ▀██████▀          
FRM
}

f15() {
	cat <<'FRM'
                          
                   ▄▄     
               ▄███████▄▄ 
               ████▀▀████ 
               ██████████ 
             ▄▄███████▀▀  
       ▄████████          
      ████▀█████          
      ██████████          
       ▀▀████▀▀           
FRM
}

f16() {
	cat <<'FRM'
                          
                          
                 ▄▄███▄▄  
               ▄█████████▄
               ███████████
        ▄▄▄▄▄▄▄█████████▀ 
     ▄██████████          
     ████▀▀████           
     ██████████           
       ▀▀██▀▀             
FRM
}

f17() {
	cat <<'FRM'
                          
                          
                   ▄▄▄▄   
                ▄████████▄
                ██████████
     ▄▄████▄▄▄▄██████████▀
    ███████████▀ ▀▀▀▀▀▀▀  
    ████  ▄████           
    ▀████████▀            
       ▀▀▀▀▀              
FRM
}

f18() {
	cat <<'FRM'
                          
                          
                          
                  ▄████▄▄ 
        ▄       ▄█████████
    ▄████████▄▄▄██████████
   ████▀▀▀█████▀▀▀▀████▀▀ 
   ████▄▄▄████            
    ▀███████▀             
                          
FRM
}

f19() {
	cat <<'FRM'
                          
                          
                          
                   ▄▄▄▄▄  
    ▄▄▄▄▄▄▄▄     ▄███████▄
  ▄███████████▄▄██████████
  ████    ████▀ ▀▀██████▀ 
  ▀████▄▄████▀            
    ▀▀████▀▀              
                          
FRM
}

f20() {
	cat <<'FRM'
                          
                          
                          
                          
   ▄███████▄▄    ▄▄█████▄ 
 ▄███▀▀▀▀▀████▄▄██████████
 ▀███▄   ▄████▀ ▀████████▀
  ▀█████████▀      ▀▀▀▀▀  
     ▀▀▀▀▀                
                          
FRM
}

f21() {
	cat <<'FRM'
                          
                          
                          
    ▄▄▄▄▄▄▄               
  ██████████▄     ▄▄▄▄▄▄  
 ████    ▀████▄▄▄████████ 
 ▀███▄▄▄▄████▀▀▀▀████████ 
   ▀██████▀▀      ▀▀██▀▀  
                          
                          
FRM
}

f22() {
	cat <<'FRM'
                          
                          
                          
  ▄████████▄              
 ████▀▀▀▀████▄     ▄▄▄    
 ████    ▄█████▄▄███████▄ 
 ▀███████████▀▀▀█████████ 
   ▀▀▀▀▀▀▀▀      ▀▀████▀  
                          
                          
FRM
}

f23() {
	cat <<'FRM'
                          
                          
    ▄▄▄▄▄▄▄               
 ▄██████████▄             
 ████    ▀████            
 ████▄▄ ▄▄█████▄▄▄█████▄  
  ▀█████████▀▀▀▀████████  
     ▀▀▀▀        ▀█████▀  
                          
                          
FRM
}

f24() {
	cat <<'FRM'
                          
                          
   ▄███████▄              
 ▄████▀▀▀████▄            
 ████     ████▄           
 ▀████▄▄▄██████▄▄▄████▄   
   ▀███████▀▀▀ ▀████████  
                ▀██████▀  
                   ▀▀     
                          
FRM
}

f25() {
	cat <<'FRM'
                          
     ▄▄▄▄▄                
  ▄█████████▄             
 ████▀▀  ▀████            
 ████▄    █████           
  ▀████████████▄▄▄▄▄▄▄    
    ▀▀▀█▀▀▀▀▀▀ ████████▄  
                ███████   
                  ▀▀▀     
                          
FRM
}

f26() {
	cat <<'FRM'
                          
    ▄▄▄██▄▄▄              
  ▄██████████▄            
 ████▀    ▀████           
 ▀████▄  ▄▄████           
  ▀█████████████▄▄▄▄▄▄    
     ▀▀▀▀▀▀▀▀  ████████   
                ███████   
                 ▀▀▀▀     
                          
FRM
}

f27() {
	cat <<'FRM'
                          
    ▄██████▄▄             
  ▄████▀▀▀████▄           
  ████     ████           
  ████▄▄▄▄▄████           
   ▀████████████▄▄▄▄▄     
       ▀▀▀▀    ███████▄   
               ▀██████▀   
                 ▀▀▀▀     
                          
FRM
}

f28() {
	cat <<'FRM'
                          
   ▄▄███████▄▄            
  ▄████▀▀▀▀████           
  ████     ████▄          
  ▀████▄▄▄▄█████          
   ▀▀███████████▄▄▄▄      
               ███████    
               ▀██████    
                 ▀▀▀▀     
                          
FRM
}

f29() {
	cat <<'FRM'
        ▄▄                
   ▄▄████████▄            
  ▄████▀▀▀▀████           
  ████     ▄████          
  ▀████▄▄▄██████          
    ▀███████████▄▄▄▄      
               ███████    
               ███████    
                ▀▀▀▀▀     
                          
FRM
}

f30() {
	cat <<'FRM'
       ▄▄▄▄               
   ▄▄████████▄            
  ▄████▀▀▀▀████▄          
  ████     ▄████          
  ▀████▄▄▄██████          
    ▀▀██████████▄▄▄▄      
               ██████▄    
               ███████    
                ▀▀▀▀▀     
                          
FRM
}

instantos_logo_animation() {
	esc=$(printf '\033')
	csi="${esc}["
	reset="${csi}0m"
	bold="${csi}1m"
	dim="${csi}2m"
	white="${csi}1;37m"
	clear_home="${csi}H"
	clear_screen="${csi}2J${csi}H"
	hide="${csi}?25l"
	show="${csi}?25h"

	# Rainbow spectrum palette
	c_red="${csi}38;5;196m"
	c_orange="${csi}38;5;208m"
	c_yellow="${csi}38;5;226m"
	c_green="${csi}38;5;46m"
	c_cyan="${csi}38;5;51m"
	c_blue="${csi}38;5;39m"
	c_purple="${csi}38;5;135m"
	c_magenta="${csi}38;5;201m"

	if [ ! -t 1 ] || [ "${TERM:-}" = "dumb" ]; then
		printf "%s" "$white"
		f30
		printf "%s" "$reset"
		printf "\n%s  instantOS Installer%s\n" "$bold" "$reset"
		printf "%s  Arch Linux, but instant%s\n\n" "$dim" "$reset"
		return 0
	fi

	cleanup_anim() {
		printf "%s%s\n" "$reset" "$show"
	}
	trap cleanup_anim INT TERM

	printf "%s%s" "$hide" "$clear_screen"

	# Play rotating and blob-morphing animation in white (2 full cycles)
	loop=1
	while [ "$loop" -le 2 ]; do
		frame_idx=1
		while [ "$frame_idx" -le 30 ]; do
			printf "%s%s" "$clear_home" "$white"
			eval "f$frame_idx"
			sleep 0.035
			frame_idx=$((frame_idx + 1))
		done
		loop=$((loop + 1))
	done

	# Rainbow spectrum flash on final resting logo
	for color in "$c_red" "$c_orange" "$c_yellow" "$c_green" "$c_cyan" "$c_blue" "$c_purple" "$c_magenta" "$white"; do
		printf "%s%s" "$clear_home" "$color"
		f30
		sleep 0.02
	done

	printf "%s" "$reset"
	printf "\n%s  instantOS Installer%s\n" "$bold" "$reset"
	printf "%s  Arch Linux, but instant%s\n\n" "$dim" "$reset"
	printf "%s" "$show"

	trap - INT TERM
}

parse_args "$@"

instantos_logo_animation

if [ "$ONLY_ANIMATION" -eq 1 ]; then
	exit 0
fi

if [ "$(id -u)" -ne 0 ]; then
	echo "Run this installer as root." >&2
	exit 1
fi

for command in curl mktemp pacman pacman-key; do
	if ! command -v "$command" >/dev/null 2>&1; then
		echo "Required command not found: $command" >&2
		exit 1
	fi
done

echo "Preparing the Arch Linux package keyring..."
pacman-key --init
pacman-key --populate archlinux
pacman -Sy --needed archlinux-keyring --noconfirm

installer_script=$(mktemp)
cleanup() {
	rm -f "$installer_script"
}
trap cleanup EXIT HUP INT TERM

echo "Installing the latest instantCLI release..."
curl -fsSL https://raw.githubusercontent.com/instantOS/instantCLI/main/scripts/install.sh \
	-o "$installer_script"
INSTALL_DIR=/usr/local/bin sh "$installer_script"
cleanup
trap - EXIT HUP INT TERM

exec /usr/local/bin/ins arch install
