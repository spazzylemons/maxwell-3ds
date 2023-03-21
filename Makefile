TARGET := maxwell.elf

BUILDDIR := build
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
	-L$(DEVKITPRO)/libctru/lib

LDFLAGS := \
	-specs=3dsx.specs \
	-lcitro3d \
	-lctru \
	-lm

CC := $(DEVKITARM)/bin/arm-none-eabi-gcc

CFILES := $(SRCDIR)/main.c
OFILES := $(CFILES:$(SRCDIR)/%.c=$(BUILDDIR)/%.o)

.PHONY: all clean run

all: $(TARGET)

$(TARGET): $(OFILES)
	$(CC) $(CFLAGS) -o $@ $(OFILES) $(LDFLAGS)

$(BUILDDIR)/%.d: $(SRCDIR)/%.c
	mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MM -MT $(<:$(SRCDIR)/%.c=$(BUILDDIR)/%.o) $< -MF $@

$(BUILDDIR)/%.o: $(SRCDIR)/%.c
	mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -rf $(BUILDDIR) $(TARGET)

run: $(TARGET)
	citra-qt $(TARGET)
