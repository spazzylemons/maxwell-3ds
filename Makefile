TARGET := maxwell.3dsx
BINARY := maxwell.elf

BUILDDIR := build
GFXDIR := gfx
SRCDIR := src

CFLAGS := \
	-Wall \
	-Oz \
	-flto \
	-mword-relocations \
	-ffunction-sections \
	-march=armv6k \
	-mtune=mpcore \
	-mfloat-abi=hard \
	-mtp=soft \
	-D__3DS__ \
	-I$(DEVKITPRO)/libctru/include \
	-I$(BUILDDIR) \
	-L$(DEVKITPRO)/libctru/lib

LDFLAGS := \
	-specs=3dsx.specs \
	-lcitro2d \
	-lcitro3d \
	-lctru \
	-lm

CC := $(DEVKITARM)/bin/arm-none-eabi-gcc
AS := $(DEVKITARM)/bin/arm-none-eabi-as

T3SFILES := $(GFXDIR)/body.t3s $(GFXDIR)/whiskers.t3s

CFILES := $(SRCDIR)/main.c $(SRCDIR)/maxwell.c
OFILES := $(CFILES:$(SRCDIR)/%.c=$(BUILDDIR)/%.o) $(BUILDDIR)/shader.o $(T3SFILES:$(GFXDIR)/%.t3s=$(BUILDDIR)/%.o)

PICAFILE := $(SRCDIR)/shader.v.pica

define binary_file
	bin2s $(1) -H $(2).h | $(AS) -o $(2).o
endef

.PHONY: all clean run

all: $(TARGET)

$(TARGET): $(BINARY)

$(BINARY): $(OFILES)
	$(CC) $(CFLAGS) -o $@ $(OFILES) $(LDFLAGS)

%.3dsx: %.elf
	3dsxtool $< $@

$(SRCDIR)/main.c: $(BUILDDIR)/shader.h $(T3SFILES:$(GFXDIR)/%.t3s=$(BUILDDIR)/%.h)

$(BUILDDIR)/shader.o $(BUILDDIR)/shader.h: $(PICAFILE)
	mkdir -p $(dir $@)
	picasso $(PICAFILE) -o $(BUILDDIR)/shader.shbin
	$(call binary_file,$(BUILDDIR)/shader.shbin,$*)

$(BUILDDIR)/%.o $(BUILDDIR)/%.h: $(GFXDIR)/%.t3s $(GFXDIR)/%.png
	mkdir -p $(dir $@)
	tex3ds -i $< -d $(BUILDDIR)/$*.d -o $(BUILDDIR)/$*.t3x
	$(call binary_file,$(BUILDDIR)/$*.t3x,$(BUILDDIR)/$*)

$(BUILDDIR)/%.d: $(SRCDIR)/%.c
	mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MM -MT $(<:$(SRCDIR)/%.c=$(BUILDDIR)/%.o) $< -MF $@

$(BUILDDIR)/%.o: $(SRCDIR)/%.c
	mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -rf $(BUILDDIR) $(BINARY) $(TARGET)

run: $(TARGET)
	citra-qt $(TARGET)
