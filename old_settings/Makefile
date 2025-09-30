export PREFIX := /usr/

SUBDIRS := utils/ data/default/

.PHONY: all
all:
	$(info Usage: make install [PREFIX=/usr/])
	true

.PHONY: $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) install -C $@

.PHONY: install
install: data/filetypes settings.sh instantsettings.desktop instantcontrolcenter.desktop $(SUBDIRS)
	$(info "INFO: install PREFIX: $(PREFIX)")
	mkdir -p $(DESTDIR)$(PREFIX)share/instantsettings $(DESTDIR)$(PREFIX)share/applications/
	install -Dm 755 settings.sh $(DESTDIR)$(PREFIX)bin/instantsettings
	install -m 644 instantsettings.desktop $(DESTDIR)$(PREFIX)share/applications/
	install -m 644 data/filetypes $(DESTDIR)$(PREFIX)share/instantsettings/data/filetypes
	install -m 644 instantcontrolcenter.desktop $(DESTDIR)$(PREFIX)share/applications/

.PHONY: uninstall
uninstall:
	rm -r $(DESTDIR)$(PREFIX)share/instantsettings
	rm -f $(DESTDIR)$(PREFIX)bin/instantsettings
	rm -f $(DESTDIR)$(PREFIX)share/applications/instantsettings.desktop
	rm -f $(DESTDIR)$(PREFIX)share/applications/instantcontrolcenter.desktop

