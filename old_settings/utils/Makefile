.PHONY: all
all:
	$(info make install [PREFIX=/usr/])
	true

.PHONY: install
install: functions.sh
	mkdir -p $(DESTDIR)$(PREFIX)share/instantsettings/utils
	for x in $^; do install -Dm 755 "$$x" "$(DESTDIR)$(PREFIX)share/instantsettings/utils/$$x"; done

