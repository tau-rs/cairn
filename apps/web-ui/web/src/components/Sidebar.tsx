import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../app/cairnStore";
import { noteUrl, tagUrl } from "../app/routes";
import { FolderTree } from "./tree/FolderTreeView";
import { TagsPanel } from "./tags/TagsPanel";
import { SlotRenderer } from "./plugins/SlotRenderer";

export function Sidebar() {
  const navigate = useNavigate();
  const actions = useActions();
  const notePaths = useCairn((s) => s.notePaths);
  const activePath = useCairn((s) => s.activePath);
  const tags = useCairn((s) => s.tags);
  const activeTag = useCairn((s) => s.activeTag);
  const treeStyles = useCairn((s) => s.treeStyles);

  return (
    <>
      <FolderTree
        paths={notePaths}
        activePath={activePath}
        onOpen={(p) => navigate(noteUrl(p))}
        onOpenToSide={actions.openToSide}
        onDelete={actions.deleteNote}
        onRecover={actions.openRecovery}
        onRequestNew={() =>
          actions.setUi({ newNoteInitial: "", newNoteOpen: true })
        }
        onRequestNewInFolder={(folder) =>
          actions.setUi({ newNoteInitial: folder + "/", newNoteOpen: true })
        }
        onApplyRenames={actions.applyRenames}
        styles={treeStyles}
        onSetStyle={actions.setTreeStyle}
        onRemapFolderStyles={actions.remapFolderStyles}
      />
      <TagsPanel
        tags={tags}
        activeTag={activeTag}
        onSelect={(t) => navigate(tagUrl(t))}
      />
      <SlotRenderer slot="sidebar.section" />
    </>
  );
}
