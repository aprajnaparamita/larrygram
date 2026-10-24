# Parity matrix: ui-spec controls

Every Fluent control id in `ui-spec/panel-2016.yaml` and `ui-spec/panel-365.yaml`, with a status of `implemented`, `stub`, or `out-of-scope`. Sorted by `(status, id)`; the status order is `implemented`, `stub`, `out-of-scope`.

The status is derived, never hand-edited (ADR 0037):

- `out-of-scope` when the id is listed in `ui-spec/out-of-scope.txt` (one Fluent id per line; matched exactly or by a `*` glob);
- `implemented` when the control's command (its `command` link, else its `commands.yaml` entry) resolves in `larrygram_commands::builtins::registry()` to `CommandStatus::Implemented`;
- `stub` otherwise.

A row listed in `ui-spec/out-of-scope.txt` stays `out-of-scope` even when a command resolves: the alpha does not implement that control. Regenerate with `cargo xtask parity`; the gate's `parity` step runs `cargo xtask parity --check` and fails when this file is stale.

| id | type | label | path | status |
|----|------|-------|------|--------|
| ConnectorTool | toggleButton | ConnectorTool | tab/TabHome/GroupTools | implemented |
| ConnectorsGallery | menu | ConnectorsGallery | tab/TabDesignVisio/GroupLayoutRouting | implemented |
| Copy | button | Copy | tab/TabIconEditor/GroupIconEditClipboard | implemented |
| Cut | button | Cut | tab/TabIconEditor/GroupIconEditClipboard | implemented |
| EllipseTool | toggleButton | EllipseTool | tab/TabDeveloper/GroupShapeDesign | implemented |
| FileNewDefault | button | FileNewDefault | qat | implemented |
| FileOpenUsingBackstage | button | FileOpenUsingBackstage | qat | implemented |
| FileSave | button | FileSave | qat | implemented |
| ObjectBringForward | button | ObjectBringForward | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectBringToFront | button | ObjectBringToFront | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectBringToFrontMenu | splitButton | ObjectBringToFrontMenu | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectSendBackward | button | ObjectSendBackward | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectSendToBack | button | ObjectSendToBack | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectSendToBackMenu | splitButton | ObjectSendToBackMenu | tab/TabPictureToolsFormat/GroupPictureToolArrange | implemented |
| ObjectsGroup | button | ObjectsGroup | tab/TabDeveloper/GroupShapeDesign | implemented |
| ObjectsGroupMenu | menu | ObjectsGroupMenu | tab/TabDeveloper/GroupShapeDesign | implemented |
| ObjectsUngroup | button | ObjectsUngroup | tab/TabDeveloper/GroupShapeDesign | implemented |
| Paste | button | Paste | tab/TabIconEditor/GroupIconEditClipboard | implemented |
| PasteMenu | splitButton | PasteMenu | tab/TabHome/GroupClipboard | implemented |
| PointerTool | toggleButton | PointerTool | tab/TabInkToolsPens/GroupInkTools | implemented |
| RectangleTool | toggleButton | RectangleTool | tab/TabDeveloper/GroupShapeDesign | implemented |
| SelectAll | button | SelectAll | tab/TabIconEditor/GroupIconEditSelect | implemented |
| ShowGrid | checkBox | ShowGrid | tab/TabView/GroupViewShowHide | implemented |
| ShowRulerVisio | checkBox | ShowRulerVisio | tab/TabView/GroupViewShowHide | implemented |
| TextTool | toggleButton | TextTool | tab/TabHome/GroupTools | implemented |
| Undo | gallery | Undo | qat | implemented |
| ZoomFitToWindowVisio | button | ZoomFitToWindowVisio | tab/TabView/GroupZoom | implemented |
| AccessibilityChecker | button | AccessibilityChecker | tab/TabReviewWord/GroupAccessibility | stub |
| AddOnsMenu | menu | AddOnsMenu | tab/TabView/GroupMacros | stub |
| AlignCenter | toggleButton | AlignCenter | tab/TabHome/GroupParagraph | stub |
| AlignGallery | gallery | AlignGallery | tab/TabHome/GroupArrange | stub |
| AlignJustify | toggleButton | AlignJustify | tab/TabHome/GroupParagraph | stub |
| AlignLeft | toggleButton | AlignLeft | tab/TabHome/GroupParagraph | stub |
| AlignRight | toggleButton | AlignRight | tab/TabHome/GroupParagraph | stub |
| AllowPersonallyIdentifiableInformation | control | AllowPersonallyIdentifiableInformation | backstage/TabInfo | stub |
| AllowThemes | toggleButton | AllowThemes | tab/TabHome/GroupShapeStyles | stub |
| AppGuardTrustPromotion | button | AppGuardTrustPromotion | backstage/TabSave | stub |
| ArcTool | toggleButton | ArcTool | tab/TabDeveloper/GroupShapeDesign | stub |
| AreaSelect | toggleButton | AreaSelect | tab/TabHome/GroupEditing | stub |
| ArrowsMore | button | ArrowsMore | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| AttachDataSource | button | AttachDataSource | tab/TabData/GroupCreateFromData | stub |
| AutoConnect | checkBox | AutoConnect | tab/TabView/GroupVisualAids | stub |
| AutoSizePage | toggleButton | AutoSizePage | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| AutomaticResize | menu | AutomaticResize | tab/TabFormat/GroupContainerSize | stub |
| AutomaticResizeAlwaysFitToContents | toggleButton | AutomaticResizeAlwaysFitToContents | tab/TabFormat/GroupContainerSize | stub |
| AutomaticResizeExpandAsNeeded | toggleButton | AutomaticResizeExpandAsNeeded | tab/TabFormat/GroupContainerSize | stub |
| AutomaticResizeNone | toggleButton | AutomaticResizeNone | tab/TabFormat/GroupContainerSize | stub |
| BackgroundColor | gallery | BackgroundColor | tab/TabDesignVisio/GroupBackgrounds | stub |
| BackgroundPageInsert | button | BackgroundPageInsert | tab/TabInsert/GroupInsertPages | stub |
| BackgroundsGallery | gallery | BackgroundsGallery | tab/TabDesignVisio/GroupBackgrounds | stub |
| BevelShapeGallery | gallery | BevelShapeGallery | tab/TabHome/GroupShapeStyles | stub |
| BlankPageInsertMenu | splitButton | BlankPageInsertMenu | tab/TabInsert/GroupInsertPages | stub |
| BlankPageInsertVisio | button | BlankPageInsertVisio | tab/TabInsert/GroupInsertPages | stub |
| Bold | toggleButton | Bold | tab/TabHome/GroupFont | stub |
| BordersAndTitlesGallery | gallery | BordersAndTitlesGallery | tab/TabDesignVisio/GroupBackgrounds | stub |
| Bullets | toggleButton | Bullets | tab/TabHome/GroupParagraph | stub |
| ButtonCategoryCloudServices | category | ButtonCategoryCloudServices | backstage/TabRecent | stub |
| ButtonCategoryLocalServices | category | ButtonCategoryLocalServices | backstage/TabRecent | stub |
| ButtonCategoryRecentDocuments | category | ButtonCategoryRecentDocuments | backstage/TabRecent | stub |
| ButtonGroupCloudServices | control | ButtonGroupCloudServices | backstage/TabRecent | stub |
| ButtonGroupItemSaveCloudServices | control | ButtonGroupItemSaveCloudServices | backstage/TabSave | stub |
| ButtonGroupItemSaveLocalServices | control | ButtonGroupItemSaveLocalServices | backstage/TabSave | stub |
| ButtonGroupLocalServices | control | ButtonGroupLocalServices | backstage/TabRecent | stub |
| ButtonGroupRecentDocuments | control | ButtonGroupRecentDocuments | backstage/TabRecent | stub |
| ButtonGroupsSave | control | ButtonGroupsSave | backstage/TabSave | stub |
| ButtonOpenBrowse | button | ButtonOpenBrowse | backstage/TabRecent | stub |
| ButtonTaskDynamicServiceProvider | task | ButtonTaskDynamicServiceProvider | backstage/TabSave | stub |
| ButtonTaskFixItHub | task | ButtonTaskFixItHub | backstage/TabRecent | stub |
| ButtonTaskRecentDocuments | task | ButtonTaskRecentDocuments | backstage/TabRecent | stub |
| ButtonTaskSharedWithMe | task | ButtonTaskSharedWithMe | backstage/TabRecent | stub |
| CadDrawing | button | CadDrawing | tab/TabInsert/GroupInsertIllustrations | stub |
| CalloutGallery | gallery | CalloutGallery | tab/TabInsert/GroupDiagramParts | stub |
| ChangeCaseDialogClassic | menu | ChangeCaseDialogClassic | tab/TabHome/GroupFont | stub |
| ChartInsert | button | ChartInsert | tab/TabInsert/GroupInsertIllustrations | stub |
| ClearIconColors | button | ClearIconColors | tab/TabIconEditor/GroupIconEditClipboard | stub |
| ClipArtInsertDialog | button | ClipArtInsertDialog | tab/TabInsert/GroupInsertIllustrations | stub |
| CloudFixItHub | group | CloudFixItHub | backstage/TabRecent | stub |
| CloudSkyDriveUpsellGroup | group | CloudSkyDriveUpsellGroup | backstage/TabSave | stub |
| CommentingPane | toggleButton | CommentingPane | tab/TabReviewWord/GroupComments | stub |
| CommentsPaneSplitButton | toggleButton | CommentsPaneSplitButton | tab/TabReviewWord/GroupComments | stub |
| ConnectionPointTool | toggleButton | ConnectionPointTool | tab/TabHome/GroupTools | stub |
| ConnectionPoints | checkBox | ConnectionPoints | tab/TabView/GroupVisualAids | stub |
| ConnectorCurvedLines | toggleButton | ConnectorCurvedLines | tab/TabDesignVisio/GroupLayoutRouting | stub |
| ConnectorRightAngle | toggleButton | ConnectorRightAngle | tab/TabDesignVisio/GroupLayoutRouting | stub |
| ConnectorShowLineJumps | toggleButton | ConnectorShowLineJumps | tab/TabDesignVisio/GroupLayoutRouting | stub |
| ConnectorStraightLines | toggleButton | ConnectorStraightLines | tab/TabDesignVisio/GroupLayoutRouting | stub |
| ContactUs | button | ContactUs | tab/HelpTab/GroupHelpAndSupport | stub |
| ContainerDisband | button | ContainerDisband | tab/TabFormat/GroupMembership | stub |
| ContainerGallery | gallery | ContainerGallery | tab/TabInsert/GroupDiagramParts | stub |
| ContainerHeadingStyles | gallery | ContainerHeadingStyles | tab/TabFormat/GroupContainerStyles | stub |
| ContainerLock | toggleButton | ContainerLock | tab/TabFormat/GroupMembership | stub |
| ContainerMargins | gallery | ContainerMargins | tab/TabFormat/GroupContainerSize | stub |
| ContainerStyles | gallery | ContainerStyles | tab/TabFormat/GroupContainerStyles | stub |
| ContextMenuCommentMarker | contextMenu | ContextMenuCommentMarker | menu/ContextMenuCommentMarker | stub |
| ContextMenuConnectionPoint | contextMenu | ContextMenuConnectionPoint | menu/ContextMenuConnectionPoint | stub |
| ContextMenuDrawingPage | contextMenu | ContextMenuDrawingPage | menu/ContextMenuDrawingPage | stub |
| ContextMenuExplorerDocument | contextMenu | ContextMenuExplorerDocument | menu/ContextMenuExplorerDocument | stub |
| ContextMenuExplorerLayer | contextMenu | ContextMenuExplorerLayer | menu/ContextMenuExplorerLayer | stub |
| ContextMenuExplorerLayers | contextMenu | ContextMenuExplorerLayers | menu/ContextMenuExplorerLayers | stub |
| ContextMenuExplorerMaster | contextMenu | ContextMenuExplorerMaster | menu/ContextMenuExplorerMaster | stub |
| ContextMenuExplorerMasters | contextMenu | ContextMenuExplorerMasters | menu/ContextMenuExplorerMasters | stub |
| ContextMenuExplorerPage | contextMenu | ContextMenuExplorerPage | menu/ContextMenuExplorerPage | stub |
| ContextMenuExplorerPages | contextMenu | ContextMenuExplorerPages | menu/ContextMenuExplorerPages | stub |
| ContextMenuExplorerPatterns | contextMenu | ContextMenuExplorerPatterns | menu/ContextMenuExplorerPatterns | stub |
| ContextMenuExplorerShape | contextMenu | ContextMenuExplorerShape | menu/ContextMenuExplorerShape | stub |
| ContextMenuExplorerShapes | contextMenu | ContextMenuExplorerShapes | menu/ContextMenuExplorerShapes | stub |
| ContextMenuExplorerStyle | contextMenu | ContextMenuExplorerStyle | menu/ContextMenuExplorerStyle | stub |
| ContextMenuExplorerStyles | contextMenu | ContextMenuExplorerStyles | menu/ContextMenuExplorerStyles | stub |
| ContextMenuFullScreen | contextMenu | ContextMenuFullScreen | menu/ContextMenuFullScreen | stub |
| ContextMenuMaster | contextMenu | ContextMenuMaster | menu/ContextMenuMaster | stub |
| ContextMenuMasterExplorerDocument | contextMenu | ContextMenuMasterExplorerDocument | menu/ContextMenuMasterExplorerDocument | stub |
| ContextMenuMasterExplorerMaster | contextMenu | ContextMenuMasterExplorerMaster | menu/ContextMenuMasterExplorerMaster | stub |
| ContextMenuOleObject | contextMenu | ContextMenuOleObject | menu/ContextMenuOleObject | stub |
| ContextMenuPageNavigation | contextMenu | ContextMenuPageNavigation | menu/ContextMenuPageNavigation | stub |
| ContextMenuPageTab | contextMenu | ContextMenuPageTab | menu/ContextMenuPageTab | stub |
| ContextMenuShape | contextMenu | ContextMenuShape | menu/ContextMenuShape | stub |
| ContextMenuShape1D | contextMenu | ContextMenuShape1D | menu/ContextMenuShape1D | stub |
| ContextMenuShapeSheet | contextMenu | ContextMenuShapeSheet | menu/ContextMenuShapeSheet | stub |
| ContextMenuSpelling | contextMenu | ContextMenuSpelling | menu/ContextMenuSpelling | stub |
| ContextMenuText | contextMenu | ContextMenuText | menu/ContextMenuText | stub |
| ContextMenuTextDrag | contextMenu | ContextMenuTextDrag | menu/ContextMenuTextDrag | stub |
| ContextMenuWindowDrawingExplorer | contextMenu | ContextMenuWindowDrawingExplorer | menu/ContextMenuWindowDrawingExplorer | stub |
| ContextMenuWindowExternalData | contextMenu | ContextMenuWindowExternalData | menu/ContextMenuWindowExternalData | stub |
| ContextMenuWindowExternalDataItem | contextMenu | ContextMenuWindowExternalDataItem | menu/ContextMenuWindowExternalDataItem | stub |
| ContextMenuWindowExternalDataTab | contextMenu | ContextMenuWindowExternalDataTab | menu/ContextMenuWindowExternalDataTab | stub |
| ContextMenuWindowFormulaTracing | contextMenu | ContextMenuWindowFormulaTracing | menu/ContextMenuWindowFormulaTracing | stub |
| ContextMenuWindowIssues | contextMenu | ContextMenuWindowIssues | menu/ContextMenuWindowIssues | stub |
| ContextMenuWindowIssuesItem | contextMenu | ContextMenuWindowIssuesItem | menu/ContextMenuWindowIssuesItem | stub |
| ContextMenuWindowMasterExplorer | contextMenu | ContextMenuWindowMasterExplorer | menu/ContextMenuWindowMasterExplorer | stub |
| ContextMenuWindowPanZoom | contextMenu | ContextMenuWindowPanZoom | menu/ContextMenuWindowPanZoom | stub |
| ContextMenuWindowShapeData | contextMenu | ContextMenuWindowShapeData | menu/ContextMenuWindowShapeData | stub |
| ContextMenuWindowShapes | contextMenu | ContextMenuWindowShapes | menu/ContextMenuWindowShapes | stub |
| ContextMenuWindowShapesMoreShapes | contextMenu | ContextMenuWindowShapesMoreShapes | menu/ContextMenuWindowShapesMoreShapes | stub |
| ContextMenuWindowShapesStencil | contextMenu | ContextMenuWindowShapesStencil | menu/ContextMenuWindowShapesStencil | stub |
| ContextMenuWindowSizePosition | contextMenu | ContextMenuWindowSizePosition | menu/ContextMenuWindowSizePosition | stub |
| ContextMenuWindowStyleExplorer | contextMenu | ContextMenuWindowStyleExplorer | menu/ContextMenuWindowStyleExplorer | stub |
| ControlProperties | button | ControlProperties | tab/TabDeveloper/GroupControls | stub |
| ControlsGallery | gallery | ControlsGallery | tab/TabDeveloper/GroupControls | stub |
| CreatePPT | task | CreatePPT | backstage/TabPublish | stub |
| CreateStageViewDiagram | button | CreateStageViewDiagram | tab/TabProcess/GroupSharepointWorkflow | stub |
| CreateWord | task | CreateWord | backstage/TabPublish | stub |
| CreateWorkflowDiagram | button | CreateWorkflowDiagram | tab/TabProcess/GroupSharepointWorkflow | stub |
| CropTool | toggleButton | CropTool | tab/TabPictureToolsFormat/GroupPictureToolArrange | stub |
| DVExportVtpx | button | DVExportVtpx | tab/TabDVDesign/GroupExportDVDiagram | stub |
| DVOpenDataSource | button | DVOpenDataSource | tab/TabDVDesign/GroupSourceDataDVDiagram | stub |
| DVRelinkDataSource | button | DVRelinkDataSource | tab/TabDVDesign/GroupSourceDataDVDiagram | stub |
| DataAutomaticallyLink | button | DataAutomaticallyLink | tab/TabData/GroupAdvancedDataLinking | stub |
| DataGraphicApplyAfterLinkingData | toggleButton | DataGraphicApplyAfterLinkingData | tab/TabData/GroupAdvancedDataLinking | stub |
| DataGraphicCalloutsGallery | gallery | DataGraphicCalloutsGallery | tab/TabData/GroupDataGraphics | stub |
| DataGraphicCreateNew | button | DataGraphicCreateNew | tab/TabData/GroupAdvancedDataLinking | stub |
| DataGraphicEdit | button | DataGraphicEdit | tab/TabData/GroupAdvancedDataLinking | stub |
| DataGraphicPosition | gallery | DataGraphicPosition | tab/TabData/GroupDataGraphics | stub |
| DataGraphicPositionHorizontal | gallery | DataGraphicPositionHorizontal | tab/TabData/GroupDataGraphics | stub |
| DataGraphicPositionVertical | gallery | DataGraphicPositionVertical | tab/TabData/GroupDataGraphics | stub |
| DataGraphicsConfigure | button | DataGraphicsConfigure | tab/TabData/GroupDataGraphics | stub |
| DataGraphicsGalleryVisio | gallery | DataGraphicsGalleryVisio | tab/TabData/GroupAdvancedDataLinking | stub |
| DataOptionsPane | checkBox | DataOptionsPane | tab/TabData/GroupDataShowHide | stub |
| DataRefreshAllDialog | button | DataRefreshAllDialog | tab/TabData/GroupExternalData | stub |
| DataRefreshDialog | button | DataRefreshDialog | tab/TabData/GroupExternalData | stub |
| DesignMode | toggleButton | DesignMode | tab/TabDeveloper/GroupControls | stub |
| DeveloperDrawingTools | splitButton | DeveloperDrawingTools | tab/TabDeveloper/GroupShapeDesign | stub |
| DiagramCheck | button | DiagramCheck | tab/TabProcess/GroupDiagramValidation | stub |
| DiagramCheckMenu | splitButton | DiagramCheckMenu | tab/TabProcess/GroupDiagramValidation | stub |
| DiagramNavigationPane | toggleButton | DiagramNavigationPane | tab/TabView/GroupViewShowHide | stub |
| DocumentStencil | checkBox | DocumentStencil | tab/TabDeveloper/GroupShowHideDeveloperVisio | stub |
| DrawConvertInkToGeometry | button | DrawConvertInkToGeometry | tab/TabDrawInk/GroupDrawCreate | stub |
| DrawConvertVisioInkToText | button | DrawConvertVisioInkToText | tab/TabDrawInk/GroupDrawConvert | stub |
| DrawSelectTool | toggleButton | DrawSelectTool | tab/TabDrawInk/GroupWrite | stub |
| DrawingExplorer | checkBox | DrawingExplorer | tab/TabDeveloper/GroupShowHideDeveloperVisio | stub |
| DrawingPage | button | DrawingPage | tab/TabStencil/GroupStencilView | stub |
| DrawingToolsMenu | splitButton | DrawingToolsMenu | tab/TabHome/GroupTools | stub |
| DuplicateThisPage | button | DuplicateThisPage | tab/TabInsert/GroupInsertPages | stub |
| DynamicGrid | checkBox | DynamicGrid | tab/TabView/GroupVisualAids | stub |
| EditIcon | button | EditIcon | tab/TabStencil/GroupStencilMasterShape | stub |
| EditMaster | button | EditMaster | tab/TabStencil/GroupStencilMasterShape | stub |
| EmbellishmentGallery | gallery | EmbellishmentGallery | tab/TabDesignVisio/GroupThemeVariants | stub |
| EventMonitor | button | EventMonitor | tab/TabDeveloper/GroupSdkTools | stub |
| ExternalDataWindow | checkBox | ExternalDataWindow | tab/TabData/GroupDataShowHide | stub |
| FieldInsert | button | FieldInsert | tab/TabInsert/GroupTextBoxText | stub |
| FilePrintQuick | button | FilePrintQuick | qat | stub |
| FileSaveAsPdfOrXps | task | FileSaveAsPdfOrXps | backstage/TabPublish | stub |
| FileSendAsAttachment | button | FileSendAsAttachment | qat | stub |
| FileTypes | category | FileTypes | backstage/TabPublish | stub |
| FileVersionHistory | group | FileVersionHistory | backstage/TabInfo | stub |
| FillColorGallery | gallery | FillColorGallery | tab/TabHome/GroupShapeStyles | stub |
| FillDialog | button | FillDialog | tab/TabHome/GroupShapeStyles | stub |
| FindDialog | button | FindDialog | tab/TabHome/GroupEditing | stub |
| FindMenu | menu | FindMenu | tab/TabHome/GroupEditing | stub |
| FingerPaintingMode | toggleButton | FingerPaintingMode | tab/TabDrawInk/GroupWrite | stub |
| FitToContents | button | FitToContents | tab/TabFormat/GroupContainerSize | stub |
| FixItHub | group | FixItHub | backstage/TabRecent | stub |
| FlyoutAnchorInsertPictures | menu | FlyoutAnchorInsertPictures | tab/TabInsert/GroupInsertIllustrations | stub |
| Font | comboBox | Font | tab/TabHome/GroupFont | stub |
| FontColorGallery | gallery | FontColorGallery | tab/TabHome/GroupFont | stub |
| FontDialogVisio | buttonDialogBoxLauncher | FontDialogVisio | tab/TabHome/GroupFont | stub |
| FontSize | comboBox | FontSize | tab/TabHome/GroupFont | stub |
| FontSizeDecrease | button | FontSizeDecrease | tab/TabHome/GroupFont | stub |
| FontSizeIncrease | button | FontSizeIncrease | tab/TabHome/GroupFont | stub |
| FormatPainter | control | FormatPainter | tab/TabHome/GroupClipboard | stub |
| FreeformTool | toggleButton | FreeformTool | tab/TabDeveloper/GroupShapeDesign | stub |
| GlowsMoreOptions | button | GlowsMoreOptions | tab/TabHome/GroupShapeStyles | stub |
| GroupARMPreview | group | GroupARMPreview | backstage/TabInfo | stub |
| GroupAboutOfficeProducts | group | GroupAboutOfficeProducts | backstage/TabHelp | stub |
| GroupAccessibility | group | GroupAccessibility | tab/TabReviewWord/GroupAccessibility | stub |
| GroupAddLocation | group | GroupAddLocation | backstage/TabSave | stub |
| GroupAddLocationLocal | group | GroupAddLocationLocal | backstage/TabSave | stub |
| GroupAllAuthors | group | GroupAllAuthors | backstage/TabInfo | stub |
| GroupArrange | group | GroupArrange | tab/TabHome/GroupArrange | stub |
| GroupBackgrounds | group | GroupBackgrounds | tab/TabDesignVisio/GroupBackgrounds | stub |
| GroupBackstageIdentity | group | GroupBackstageIdentity | backstage/TabHelp | stub |
| GroupBackstagePrivacy | group | GroupBackstagePrivacy | backstage/TabHelp | stub |
| GroupBackstageSignIn | group | GroupBackstageSignIn | backstage/TabHelp | stub |
| GroupBackstageSignInDisabled | group | GroupBackstageSignInDisabled | backstage/TabHelp | stub |
| GroupCachedOfflineMergeable | group | GroupCachedOfflineMergeable | backstage/TabInfo | stub |
| GroupCheckInCheckOut | group | GroupCheckInCheckOut | backstage/TabInfo | stub |
| GroupClickToRunUpdateStatus | group | GroupClickToRunUpdateStatus | backstage/TabHelp | stub |
| GroupClipboard | group | GroupClipboard | tab/TabHome/GroupClipboard | stub |
| GroupCloudGettingStartedOpenMRUPlacesOrNavSlab | group | GroupCloudGettingStartedOpenMRUPlacesOrNavSlab | backstage/TabRecent | stub |
| GroupCode | group | GroupCode | tab/TabDeveloper/GroupCode | stub |
| GroupComments | group | GroupComments | tab/TabReviewWord/GroupComments | stub |
| GroupCompatChecker | group | GroupCompatChecker | backstage/TabInfo | stub |
| GroupContainerSize | group | GroupContainerSize | tab/TabFormat/GroupContainerSize | stub |
| GroupContainerStyles | group | GroupContainerStyles | tab/TabFormat/GroupContainerStyles | stub |
| GroupControls | group | GroupControls | tab/TabDeveloper/GroupControls | stub |
| GroupCreateFromData | group | GroupCreateFromData | tab/TabData/GroupCreateFromData | stub |
| GroupCreatePdfOrXpsDocument | group | GroupCreatePdfOrXpsDocument | backstage/TabPublish | stub |
| GroupDataGraphics | group | GroupDataGraphics | tab/TabData/GroupDataGraphics | stub |
| GroupDataShowHide | group | GroupDataShowHide | tab/TabData/GroupDataShowHide | stub |
| GroupDeprecatedAddon | group | GroupDeprecatedAddon | backstage/TabInfo | stub |
| GroupDiagramParts | group | GroupDiagramParts | tab/TabInsert/GroupDiagramParts | stub |
| GroupDiagramValidation | group | GroupDiagramValidation | tab/TabProcess/GroupDiagramValidation | stub |
| GroupDisplayData | group | GroupDisplayData | tab/TabData/GroupDisplayData | stub |
| GroupDocumentViewsVisio | group | GroupDocumentViewsVisio | tab/TabView/GroupDocumentViewsVisio | stub |
| GroupDrawConvert | group | GroupDrawConvert | tab/TabDrawInk/GroupDrawConvert | stub |
| GroupDrawCreate | group | GroupDrawCreate | tab/TabDrawInk/GroupDrawCreate | stub |
| GroupDynamicSaveCloudServiceProvider | group | GroupDynamicSaveCloudServiceProvider | backstage/TabSave | stub |
| GroupDynamicSaveLocalServiceProvider | group | GroupDynamicSaveLocalServiceProvider | backstage/TabSave | stub |
| GroupDynamicSavePinnableCloudServiceProvider | group | GroupDynamicSavePinnableCloudServiceProvider | backstage/TabSave | stub |
| GroupDynamicServiceProvider | group | GroupDynamicServiceProvider | backstage/TabSave | stub |
| GroupEditing | group | GroupEditing | tab/TabHome/GroupEditing | stub |
| GroupExportDVDiagram | group | GroupExportDVDiagram | tab/TabDVDesign/GroupExportDVDiagram | stub |
| GroupExternalData | group | GroupExternalData | tab/TabData/GroupExternalData | stub |
| GroupFileConvert | group | GroupFileConvert | backstage/TabInfo | stub |
| GroupFileProperties | group | GroupFileProperties | backstage/TabInfo | stub |
| GroupFlow | group | GroupFlow | tab/TabProcess/GroupFlow | stub |
| GroupFont | group | GroupFont | tab/TabHome/GroupFont | stub |
| GroupGettingStartedOpenMRUPlacesOrNavSlab | group | GroupGettingStartedOpenMRUPlacesOrNavSlab | backstage/TabRecent | stub |
| GroupGettingStartedSaveAsMRUCloudPlacesOrNavSlab | group | GroupGettingStartedSaveAsMRUCloudPlacesOrNavSlab | backstage/TabSave | stub |
| GroupGettingStartedSaveAsMRULocalPlacesOrNavSlab | group | GroupGettingStartedSaveAsMRULocalPlacesOrNavSlab | backstage/TabSave | stub |
| GroupGettingStartedSaveAsMRUPlacesOrNavSlab | group | GroupGettingStartedSaveAsMRUPlacesOrNavSlab | backstage/TabSave | stub |
| GroupHelpAndSupport | group | GroupHelpAndSupport | tab/HelpTab/GroupHelpAndSupport | stub |
| GroupIconEditClipboard | group | GroupIconEditClipboard | tab/TabIconEditor/GroupIconEditClipboard | stub |
| GroupIconEditClose | group | GroupIconEditClose | tab/TabIconEditor/GroupIconEditClose | stub |
| GroupIconEditPaint | group | GroupIconEditPaint | tab/TabIconEditor/GroupIconEditPaint | stub |
| GroupIconEditSelect | group | GroupIconEditSelect | tab/TabIconEditor/GroupIconEditSelect | stub |
| GroupInkClose | group | GroupInkClose | tab/TabInkToolsPens/GroupInkClose | stub |
| GroupInkConvert | group | GroupInkConvert | tab/TabInkToolsPens/GroupInkConvert | stub |
| GroupInkCreate | group | GroupInkCreate | tab/TabInkToolsPens/GroupInkCreate | stub |
| GroupInkPens | group | GroupInkPens | tab/TabInkToolsPens/GroupInkPens | stub |
| GroupInkTools | group | GroupInkTools | tab/TabInkToolsPens/GroupInkTools | stub |
| GroupInsertIllustrations | group | GroupInsertIllustrations | tab/TabInsert/GroupInsertIllustrations | stub |
| GroupInsertLinks | group | GroupInsertLinks | tab/TabInsert/GroupInsertLinks | stub |
| GroupInsertPages | group | GroupInsertPages | tab/TabInsert/GroupInsertPages | stub |
| GroupInsights | group | GroupInsights | tab/TabReviewWord/GroupInsights | stub |
| GroupItemPermissions | group | GroupItemPermissions | backstage/TabInfo | stub |
| GroupLanguage | group | GroupLanguage | tab/TabReviewWord/GroupLanguage | stub |
| GroupLayoutRouting | group | GroupLayoutRouting | tab/TabDesignVisio/GroupLayoutRouting | stub |
| GroupLocalGettingStartedOpenMRUPlacesOrNavSlab | group | GroupLocalGettingStartedOpenMRUPlacesOrNavSlab | backstage/TabRecent | stub |
| GroupMembership | group | GroupMembership | tab/TabFormat/GroupMembership | stub |
| GroupModernOpenPlaces | group | GroupModernOpenPlaces | backstage/TabRecent | stub |
| GroupNewStencil | group | GroupNewStencil | tab/TabDeveloper/GroupNewStencil | stub |
| GroupOfficeBranding | group | GroupOfficeBranding | backstage/TabHelp | stub |
| GroupOfficeInsider | group | GroupOfficeInsider | backstage/TabHelp | stub |
| GroupOfficeInsiderAbout | button | GroupOfficeInsiderAbout | backstage/TabHelp | stub |
| GroupOfficeInsiderChannel | button | GroupOfficeInsiderChannel | backstage/TabHelp | stub |
| GroupOfficeInsiderHelp | button | GroupOfficeInsiderHelp | backstage/TabHelp | stub |
| GroupOfficeInsiderRegistration | button | GroupOfficeInsiderRegistration | backstage/TabHelp | stub |
| GroupOfficeSendFrown | group | GroupOfficeSendFrown | backstage/TabOfficeFeedback | stub |
| GroupOfficeSendSmile | group | GroupOfficeSendSmile | backstage/TabOfficeFeedback | stub |
| GroupOfficeSendSuggestion | group | GroupOfficeSendSuggestion | backstage/TabOfficeFeedback | stub |
| GroupOfficeStart | group | GroupOfficeStart | backstage/TabOfficeStart | stub |
| GroupOpenPlaces | taskFormGroup | GroupOpenPlaces | backstage/TabRecent | stub |
| GroupOrgChartArrange | group | GroupOrgChartArrange | tab/TabOrgChart/GroupOrgChartArrange | stub |
| GroupOrgChartLayout | group | GroupOrgChartLayout | tab/TabOrgChart/GroupOrgChartLayout | stub |
| GroupOrgChartOrganizationData | group | GroupOrgChartOrganizationData | tab/TabOrgChart/GroupOrgChartOrganizationData | stub |
| GroupOrgChartPicture | group | GroupOrgChartPicture | tab/TabOrgChart/GroupOrgChartPicture | stub |
| GroupOrgChartShapes | group | GroupOrgChartShapes | tab/TabOrgChart/GroupOrgChartShapes | stub |
| GroupOrgChartSpacing | labelControl | GroupOrgChartSpacing | tab/TabOrgChart/GroupOrgChartArrange | stub |
| GroupParagraph | group | GroupParagraph | tab/TabHome/GroupParagraph | stub |
| GroupPens | group | GroupPens | tab/TabDrawInk/GroupPens | stub |
| GroupPictureStylesVisio | group | GroupPictureStylesVisio | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| GroupPictureToolAdjust | group | GroupPictureToolAdjust | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| GroupPictureToolArrange | group | GroupPictureToolArrange | tab/TabPictureToolsFormat/GroupPictureToolArrange | stub |
| GroupPinnableCloudSaveAsAccessibility | group | GroupPinnableCloudSaveAsAccessibility | backstage/TabSave | stub |
| GroupPinnableGettingStartedSaveAsMRUCloudPlacesOrNav | group | GroupPinnableGettingStartedSaveAsMRUCloudPlacesOrNav | backstage/TabSave | stub |
| GroupPrepareForSharing | group | GroupPrepareForSharing | backstage/TabInfo | stub |
| GroupPrintPreview | group | GroupPrintPreview | backstage/TabPrint | stub |
| GroupPrintPreviewPageSetup | group | GroupPrintPreviewPageSetup | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| GroupPrintProtectedView | group | GroupPrintProtectedView | backstage/TabPrint | stub |
| GroupPrintSettings | group | GroupPrintSettings | backstage/TabPrint | stub |
| GroupProofing | group | GroupProofing | tab/TabReviewWord/GroupProofing | stub |
| GroupReadOnly | group | GroupReadOnly | backstage/TabInfo | stub |
| GroupReadOnlyCached | group | GroupReadOnlyCached | backstage/TabInfo | stub |
| GroupReadOnlyCachedMergeable | group | GroupReadOnlyCachedMergeable | backstage/TabInfo | stub |
| GroupReadOnlyCheckOutRequired | group | GroupReadOnlyCheckOutRequired | backstage/TabInfo | stub |
| GroupReadOnlyOffline | group | GroupReadOnlyOffline | backstage/TabInfo | stub |
| GroupReadOnlyServer | group | GroupReadOnlyServer | backstage/TabInfo | stub |
| GroupRecentCloudPlaces | group | GroupRecentCloudPlaces | backstage/TabRecent | stub |
| GroupRecentDocuments | group | GroupRecentDocuments | backstage/TabRecent | stub |
| GroupRecentDocumentsNew | group | GroupRecentDocumentsNew | backstage/TabRecent | stub |
| GroupRecentLocalPlaces | group | GroupRecentLocalPlaces | backstage/TabRecent | stub |
| GroupRecentPinnableCloudPlaces | group | GroupRecentPinnableCloudPlaces | backstage/TabRecent | stub |
| GroupRecentPlaces | group | GroupRecentPlaces | backstage/TabRecent | stub |
| GroupReduceFileSize | group | GroupReduceFileSize | backstage/TabInfo | stub |
| GroupRefreshUpdate | group | GroupRefreshUpdate | tab/TabDVDesign/GroupRefreshUpdate | stub |
| GroupRemoveHiddenInformation | group | GroupRemoveHiddenInformation | backstage/TabInfo | stub |
| GroupReports | group | GroupReports | tab/TabReviewWord/GroupReports | stub |
| GroupSdkTools | group | GroupSdkTools | tab/TabDeveloper/GroupSdkTools | stub |
| GroupSecurityInformation | group | GroupSecurityInformation | backstage/TabInfo | stub |
| GroupSecurityWarning | group | GroupSecurityWarning | backstage/TabInfo | stub |
| GroupSendAsAttachment | group | GroupSendAsAttachment | backstage/TabShare | stub |
| GroupSendAsLink | group | GroupSendAsLink | backstage/TabShare | stub |
| GroupSendAsPdf | group | GroupSendAsPdf | backstage/TabShare | stub |
| GroupSendAsXps | group | GroupSendAsXps | backstage/TabShare | stub |
| GroupSendUsingEmail | group | GroupSendUsingEmail | backstage/TabShare | stub |
| GroupShapeDesign | group | GroupShapeDesign | tab/TabDeveloper/GroupShapeDesign | stub |
| GroupShapeSheetEditing | group | GroupShapeSheetEditing | tab/TabShapeSheetDesign/GroupShapeSheetEditing | stub |
| GroupShapeSheetFormulaTracing | group | GroupShapeSheetFormulaTracing | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| GroupShapeSheetRows | group | GroupShapeSheetRows | tab/TabShapeSheetDesign/GroupShapeSheetRows | stub |
| GroupShapeSheetSections | group | GroupShapeSheetSections | tab/TabShapeSheetDesign/GroupShapeSheetSections | stub |
| GroupShapeSheetView | group | GroupShapeSheetView | tab/TabShapeSheetDesign/GroupShapeSheetView | stub |
| GroupShapeStyles | group | GroupShapeStyles | tab/TabHome/GroupShapeStyles | stub |
| GroupShare | taskFormGroup | GroupShare | backstage/TabShare | stub |
| GroupShareWithPeople | group | GroupShareWithPeople | backstage/TabShare | stub |
| GroupSharedWithMeDocuments | group | GroupSharedWithMeDocuments | backstage/TabRecent | stub |
| GroupSharedWithMeDocumentsNew | group | GroupSharedWithMeDocumentsNew | backstage/TabRecent | stub |
| GroupSharepointWorkflow | group | GroupSharepointWorkflow | tab/TabProcess/GroupSharepointWorkflow | stub |
| GroupShowHideDeveloperVisio | group | GroupShowHideDeveloperVisio | tab/TabDeveloper/GroupShowHideDeveloperVisio | stub |
| GroupSourceDataDVDiagram | group | GroupSourceDataDVDiagram | tab/TabDVDesign/GroupSourceDataDVDiagram | stub |
| GroupStencil | group | GroupStencil | tab/TabStencil/GroupStencil | stub |
| GroupStencilClose | group | GroupStencilClose | tab/TabStencil/GroupStencilClose | stub |
| GroupStencilMasterShape | group | GroupStencilMasterShape | tab/TabStencil/GroupStencilMasterShape | stub |
| GroupStencilView | group | GroupStencilView | tab/TabStencil/GroupStencilView | stub |
| GroupStencilWindow | group | GroupStencilWindow | tab/TabStencil/GroupStencilWindow | stub |
| GroupSubProcess | group | GroupSubProcess | tab/TabProcess/GroupSubProcess | stub |
| GroupTextBoxText | group | GroupTextBoxText | tab/TabInsert/GroupTextBoxText | stub |
| GroupThemeVariants | group | GroupThemeVariants | tab/TabDesignVisio/GroupThemeVariants | stub |
| GroupThemesVisio | group | GroupThemesVisio | tab/TabDesignVisio/GroupThemesVisio | stub |
| GroupThemesVisioLegacy | group | GroupThemesVisioLegacy | tab/TabDesignVisio/GroupThemesVisioLegacy | stub |
| GroupThumbnail | group | GroupThumbnail | backstage/TabInfo | stub |
| GroupTools | group | GroupTools | tab/TabHome/GroupTools | stub |
| GroupUpdateChannel | group | GroupUpdateChannel | backstage/TabHelp | stub |
| GroupUpdatesAvailable | group | GroupUpdatesAvailable | backstage/TabInfo | stub |
| GroupUploadErrors | group | GroupUploadErrors | backstage/TabInfo | stub |
| GroupViewDirection | group | GroupViewDirection | tab/TabView/GroupViewDirection | stub |
| GroupViewShowHide | group | GroupViewShowHide | tab/TabView/GroupViewShowHide | stub |
| GroupVisioExport | group | GroupVisioExport | tab/TabView/GroupVisioExport | stub |
| GroupVisualAids | group | GroupVisualAids | tab/TabView/GroupVisualAids | stub |
| GroupWhatsNewInOfficeProducts | group | GroupWhatsNewInOfficeProducts | backstage/TabHelp | stub |
| GroupWindow | group | GroupWindow | tab/TabView/GroupWindow | stub |
| GroupWrite | group | GroupWrite | tab/TabDrawInk/GroupWrite | stub |
| GroupZoom | group | GroupZoom | tab/TabView/GroupZoom | stub |
| Help | button | Help | tab/HelpTab/GroupHelpAndSupport | stub |
| HelpTab | tab | HelpTab | tab/HelpTab | stub |
| HomePageSlabCreateNew | group | HomePageSlabCreateNew | backstage/PlaceTabHome | stub |
| HomePlaceRecommendedDocuments | group | HomePlaceRecommendedDocuments | backstage/PlaceTabHome | stub |
| HyperlinkInsert | button | HyperlinkInsert | tab/TabShapeSheetDesign/GroupShapeSheetEditing | stub |
| IconBucketTool | button | IconBucketTool | tab/TabIconEditor/GroupIconEditPaint | stub |
| IconEditClose | button | IconEditClose | tab/TabIconEditor/GroupIconEditClose | stub |
| IconInsertFromFile | button | IconInsertFromFile | tab/TabInsert/GroupInsertIllustrations | stub |
| IconPencilTool | button | IconPencilTool | tab/TabIconEditor/GroupIconEditPaint | stub |
| IconSelectArea | button | IconSelectArea | tab/TabIconEditor/GroupIconEditSelect | stub |
| IconSelectLasso | button | IconSelectLasso | tab/TabIconEditor/GroupIconEditSelect | stub |
| IconsAndDetails | toggleButton | IconsAndDetails | tab/TabStencil/GroupStencilView | stub |
| IconsAndNames | toggleButton | IconsAndNames | tab/TabStencil/GroupStencilView | stub |
| IconsOnly | toggleButton | IconsOnly | tab/TabStencil/GroupStencilView | stub |
| IgnoreRule | button | IgnoreRule | tab/TabProcess/GroupDiagramValidation | stub |
| IgnoreThisIssue | button | IgnoreThisIssue | tab/TabProcess/GroupDiagramValidation | stub |
| IgnoreThisIssueMenu | splitButton | IgnoreThisIssueMenu | tab/TabProcess/GroupDiagramValidation | stub |
| ImportFlowAsVisioBPMNDiagram | button | ImportFlowAsVisioBPMNDiagram | tab/TabProcess/GroupFlow | stub |
| IndentDecreaseWord | button | IndentDecreaseWord | tab/TabHome/GroupParagraph | stub |
| IndentIncreaseWord | button | IndentIncreaseWord | tab/TabHome/GroupParagraph | stub |
| InkAutoCreateShapes | checkBox | InkAutoCreateShapes | tab/TabInkToolsPens/GroupInkCreate | stub |
| InkBallpointPen | toggleButton | InkBallpointPen | tab/TabInkToolsPens/GroupInkTools | stub |
| InkColorPicker | gallery | InkColorPicker | tab/TabInkToolsPens/GroupInkPens | stub |
| InkCovertToGeometry | button | InkCovertToGeometry | tab/TabInkToolsPens/GroupInkConvert | stub |
| InkCovertToTextVisio | button | InkCovertToTextVisio | tab/TabInkToolsPens/GroupInkConvert | stub |
| InkEraser | toggleButton | InkEraser | tab/TabInkToolsPens/GroupInkTools | stub |
| InkHighlighter | toggleButton | InkHighlighter | tab/TabInkToolsPens/GroupInkTools | stub |
| InkShapeClose | button | InkShapeClose | tab/TabInkToolsPens/GroupInkCreate | stub |
| InkTool | button | InkTool | tab/TabReviewWord/GroupComments | stub |
| InkToolsClose | button | InkToolsClose | tab/TabInkToolsPens/GroupInkClose | stub |
| InsertM365Picture | button | InsertM365Picture | tab/TabInsert/GroupInsertIllustrations | stub |
| Insights | button | Insights | tab/TabReviewWord/GroupInsights | stub |
| IssuesWindow | checkBox | IssuesWindow | tab/TabProcess/GroupDiagramValidation | stub |
| Italic | toggleButton | Italic | tab/TabHome/GroupFont | stub |
| LanguageCommands | menu | LanguageCommands | tab/TabReviewWord/GroupLanguage | stub |
| LanguagePreferences | button | LanguagePreferences | tab/TabReviewWord/GroupLanguage | stub |
| LassoSelectVisio | toggleButton | LassoSelectVisio | tab/TabDrawInk/GroupWrite | stub |
| LayerDialog | button | LayerDialog | tab/TabHome/GroupEditing | stub |
| LayerPropertiesDialog | button | LayerPropertiesDialog | tab/TabHome/GroupEditing | stub |
| LayersMenu | menu | LayersMenu | tab/TabHome/GroupEditing | stub |
| LayoutApplyToShapeStyle | toggleButton | LayoutApplyToShapeStyle | tab/TabOrgChart/GroupOrgChartShapes | stub |
| LayoutMoreOptions | button | LayoutMoreOptions | tab/TabDesignVisio/GroupLayoutRouting | stub |
| LeftButtonIconColor | gallery | LeftButtonIconColor | tab/TabIconEditor/GroupIconEditPaint | stub |
| LeftButtonIconTransparent | button | LeftButtonIconTransparent | tab/TabIconEditor/GroupIconEditPaint | stub |
| LegacyMode | toggleButton | LegacyMode | tab/TabView/GroupDocumentViewsVisio | stub |
| LegendInsertGallery | gallery | LegendInsertGallery | tab/TabData/GroupDisplayData | stub |
| LineArrowsGallery | gallery | LineArrowsGallery | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LineColorGallery | gallery | LineColorGallery | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LineDialog | button | LineDialog | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LinePatternGallery | gallery | LinePatternGallery | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LineStylesDialog | button | LineStylesDialog | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LineThickness | gallery | LineThickness | tab/TabInkToolsPens/GroupInkPens | stub |
| LineTool | toggleButton | LineTool | tab/TabDeveloper/GroupShapeDesign | stub |
| LineWeightGallery | gallery | LineWeightGallery | tab/TabPictureToolsFormat/GroupPictureStylesVisio | stub |
| LoadingCloudServicesGroup | group | LoadingCloudServicesGroup | backstage/TabSave | stub |
| LoadingServicesGroup | group | LoadingServicesGroup | backstage/TabSave | stub |
| LocalSave | category | LocalSave | backstage/TabSave | stub |
| ManagePrivacy | button | ManagePrivacy | backstage/TabHelp | stub |
| ManageServices | group | ManageServices | backstage/TabHelp | stub |
| MasterExplorer | checkBox | MasterExplorer | tab/TabDeveloper/GroupShowHideDeveloperVisio | stub |
| MasterProperties | button | MasterProperties | tab/TabStencil/GroupStencilMasterShape | stub |
| ModernSaveGroup | group | ModernSaveGroup | backstage/TabSave | stub |
| MoreDataGraphics | button | MoreDataGraphics | tab/TabData/GroupDataGraphics | stub |
| MorePageSizes | button | MorePageSizes | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| Move | labelControl | Move | tab/TabOrgChart/GroupOrgChartArrange | stub |
| NamesOnly | toggleButton | NamesOnly | tab/TabStencil/GroupStencilView | stub |
| NamesUnderIcons | toggleButton | NamesUnderIcons | tab/TabStencil/GroupStencilView | stub |
| NewBlankStencil | button | NewBlankStencil | tab/TabStencil/GroupStencil | stub |
| NewBlankStencilMetric | button | NewBlankStencilMetric | tab/TabStencil/GroupStencil | stub |
| NewBlankStencilUS | button | NewBlankStencilUS | tab/TabStencil/GroupStencil | stub |
| NewMaster | button | NewMaster | tab/TabStencil/GroupStencilMasterShape | stub |
| ObjectEffectGlowGallery | gallery | ObjectEffectGlowGallery | tab/TabHome/GroupShapeStyles | stub |
| ObjectEffectShadowGallery | gallery | ObjectEffectShadowGallery | tab/TabHome/GroupShapeStyles | stub |
| ObjectEffectSoftEdgesGallery | gallery | ObjectEffectSoftEdgesGallery | tab/TabHome/GroupShapeStyles | stub |
| ObjectFormatDialog | buttonDialogBoxLauncher | ObjectFormatDialog | tab/TabHome/GroupShapeStyles | stub |
| ObjectRotateGallery | gallery | ObjectRotateGallery | tab/TabPictureToolsFormat/GroupPictureToolArrange | stub |
| ObjectsAddToGroup | button | ObjectsAddToGroup | tab/TabDeveloper/GroupShapeDesign | stub |
| ObjectsConvertToGroup | button | ObjectsConvertToGroup | tab/TabDeveloper/GroupShapeDesign | stub |
| ObjectsRemoveFromGroup | button | ObjectsRemoveFromGroup | tab/TabDeveloper/GroupShapeDesign | stub |
| OfficeFeedbackHelpTab | button | OfficeFeedbackHelpTab | tab/HelpTab/GroupHelpAndSupport | stub |
| OfficeFeedbackIcon | button | OfficeFeedbackIcon | tab/HelpTab/GroupHelpAndSupport | stub |
| OleObjectctInsert | button | OleObjectctInsert | tab/TabInsert/GroupTextBoxText | stub |
| OpenStencil | button | OpenStencil | tab/TabStencil/GroupStencil | stub |
| OperationsMenu | menu | OperationsMenu | tab/TabDeveloper/GroupShapeDesign | stub |
| OrgChartArrangeSubordinates | buttonDialogBoxLauncher | OrgChartArrangeSubordinates | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartChangeStyle | gallery | OrgChartChangeStyle | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartCopyCreate | toggleButton | OrgChartCopyCreate | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartDataCompare | button | OrgChartDataCompare | tab/TabOrgChart/GroupOrgChartOrganizationData | stub |
| OrgChartDataExport | button | OrgChartDataExport | tab/TabOrgChart/GroupOrgChartOrganizationData | stub |
| OrgChartDataImport | button | OrgChartDataImport | tab/TabOrgChart/GroupOrgChartOrganizationData | stub |
| OrgChartDataOptionsDisplay | buttonDialogBoxLauncher | OrgChartDataOptionsDisplay | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartFitToPage | button | OrgChartFitToPage | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartIncreaseHeight | button | OrgChartIncreaseHeight | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartIncreaseWidth | button | OrgChartIncreaseWidth | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartLayoutGallery | gallery | OrgChartLayoutGallery | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartLayoutSpacingChange | buttonDialogBoxLauncher | OrgChartLayoutSpacingChange | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartPictureChange | button | OrgChartPictureChange | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPictureDelete | button | OrgChartPictureDelete | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPictureHide | button | OrgChartPictureHide | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPictureInsert | splitButton | OrgChartPictureInsert | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPictureInsertMultiple | button | OrgChartPictureInsertMultiple | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPictureInsertOne | button | OrgChartPictureInsertOne | tab/TabOrgChart/GroupOrgChartPicture | stub |
| OrgChartPositionTypeChange | button | OrgChartPositionTypeChange | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartReduceHeight | button | OrgChartReduceHeight | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartReduceWidth | button | OrgChartReduceWidth | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartRelayout | button | OrgChartRelayout | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartReportMoveCloser | button | OrgChartReportMoveCloser | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartReportMoveFurther | button | OrgChartReportMoveFurther | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartReportMoveLeft | button | OrgChartReportMoveLeft | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartReportMoveRight | button | OrgChartReportMoveRight | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartShapeHeight | labelControl | OrgChartShapeHeight | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartShapeWidth | labelControl | OrgChartShapeWidth | tab/TabOrgChart/GroupOrgChartShapes | stub |
| OrgChartSubordinatesExpand | toggleButton | OrgChartSubordinatesExpand | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OrgChartSubordinatesShowHide | button | OrgChartSubordinatesShowHide | tab/TabOrgChart/GroupOrgChartArrange | stub |
| OrgChartSynchronize | splitButton | OrgChartSynchronize | tab/TabOrgChart/GroupOrgChartLayout | stub |
| OutlineWeightGallery | gallery | OutlineWeightGallery | tab/TabInkToolsPens/GroupInkPens | stub |
| PageFitToDrawing | button | PageFitToDrawing | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| PageOrientation | gallery | PageOrientation | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| PageRelayoutGallery | gallery | PageRelayoutGallery | tab/TabDesignVisio/GroupLayoutRouting | stub |
| PageSetupDialogVisio | buttonDialogBoxLauncher | PageSetupDialogVisio | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| PageSetupLayoutAndRoutingDialog | buttonDialogBoxLauncher | PageSetupLayoutAndRoutingDialog | tab/TabDesignVisio/GroupLayoutRouting | stub |
| PageSetupPageDialog | buttonDialogBoxLauncher | PageSetupPageDialog | tab/TabDesignVisio/GroupBackgrounds | stub |
| PageSize | gallery | PageSize | tab/TabDesignVisio/GroupPrintPreviewPageSetup | stub |
| PanAndZoomWindow | toggleButton | PanAndZoomWindow | tab/TabView/GroupViewShowHide | stub |
| ParagraphDialogVisio | buttonDialogBoxLauncher | ParagraphDialogVisio | tab/TabHome/GroupParagraph | stub |
| PasteAsHyperlink | button | PasteAsHyperlink | tab/TabHome/GroupClipboard | stub |
| PasteDuplicate | button | PasteDuplicate | tab/TabHome/GroupClipboard | stub |
| PasteSpecialDialog | button | PasteSpecialDialog | tab/TabHome/GroupClipboard | stub |
| PenAddFavorite | button | PenAddFavorite | tab/TabDrawInk/GroupPens | stub |
| PenPressure | toggleButton | PenPressure | tab/TabInkToolsPens/GroupInkPens | stub |
| PencilTool | toggleButton | PencilTool | tab/TabDeveloper/GroupShapeDesign | stub |
| PensGallery | gallery | PensGallery | tab/TabDrawInk/GroupPens | stub |
| PersistentEvents | button | PersistentEvents | tab/TabDeveloper/GroupSdkTools | stub |
| Personalization | group | Personalization | backstage/TabHelp | stub |
| PictureAutoBalance | button | PictureAutoBalance | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureBrightnessGallery | gallery | PictureBrightnessGallery | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureCompress | button | PictureCompress | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureContrastGallery | gallery | PictureContrastGallery | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureCorrectionsDialog | button | PictureCorrectionsDialog | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureFormatDialogVisio | buttonDialogBoxLauncher | PictureFormatDialogVisio | tab/TabPictureToolsFormat/GroupPictureToolAdjust | stub |
| PictureInsertFromFile | button | PictureInsertFromFile | tab/TabInsert/GroupInsertIllustrations | stub |
| PinnableGroupCloudGettingStartedOpenMRUPlacesOrNavSlab | group | PinnableGroupCloudGettingStartedOpenMRUPlacesOrNavSlab | backstage/TabRecent | stub |
| PlaceTabHome | tab | PlaceTabHome | backstage/PlaceTabHome | stub |
| PointerModeOptions | gallery | PointerModeOptions | qat | stub |
| PositionMenu | gallery | PositionMenu | tab/TabHome/GroupArrange | stub |
| PrepareVisioDiagramForExportToFlow | button | PrepareVisioDiagramForExportToFlow | tab/TabProcess/GroupFlow | stub |
| PrintShapeSheet | button | PrintShapeSheet | tab/TabDeveloper/GroupSdkTools | stub |
| PublishGroup | taskFormGroup | PublishGroup | backstage/TabPublish | stub |
| PublishOptions | group | PublishOptions | backstage/TabInfo | stub |
| QuickConnectorTool | toggleButton | QuickConnectorTool | tab/TabInsert/GroupDiagramParts | stub |
| RecorderResume | button | RecorderResume | tab/TabDeveloper/GroupCode | stub |
| RecorderStopVisio | button | RecorderStopVisio | tab/TabDeveloper/GroupCode | stub |
| RedoOrRepeat | button | RedoOrRepeat | qat | stub |
| ReflectionGallery | gallery | ReflectionGallery | tab/TabHome/GroupShapeStyles | stub |
| ReflectionsMoreOptions | button | ReflectionsMoreOptions | tab/TabHome/GroupShapeStyles | stub |
| RefreshDiagram | button | RefreshDiagram | tab/TabDVDesign/GroupRefreshUpdate | stub |
| RefreshMenu | splitButton | RefreshMenu | tab/TabData/GroupExternalData | stub |
| RemoveTheme | button | RemoveTheme | tab/TabHome/GroupShapeStyles | stub |
| ReplaceDialog | button | ReplaceDialog | tab/TabHome/GroupEditing | stub |
| ReplaceShape | gallery | ReplaceShape | tab/TabHome/GroupEditing | stub |
| ResearchPane | toggleButton | ResearchPane | tab/TabReviewWord/GroupProofing | stub |
| ReviewNewComment | button | ReviewNewComment | tab/TabReviewWord/GroupComments | stub |
| RightButtonIconColor | gallery | RightButtonIconColor | tab/TabIconEditor/GroupIconEditPaint | stub |
| RightButtonIconTransparent | button | RightButtonIconTransparent | tab/TabIconEditor/GroupIconEditPaint | stub |
| RotateDiagram | gallery | RotateDiagram | tab/TabHome/GroupArrange | stub |
| RulerAndGridDialog | buttonDialogBoxLauncher | RulerAndGridDialog | tab/TabView/GroupViewShowHide | stub |
| RulesImportFrom | gallery | RulesImportFrom | tab/TabProcess/GroupDiagramValidation | stub |
| RulesToCheck | menu | RulesToCheck | tab/TabProcess/GroupDiagramValidation | stub |
| Save | category | Save | backstage/TabSave | stub |
| SaveCloudServices | category | SaveCloudServices | backstage/TabSave | stub |
| SaveDrawing | group | SaveDrawing | backstage/TabPublish | stub |
| SaveGroup | taskFormGroup | SaveGroup | backstage/TabSave | stub |
| ScreenTip | button | ScreenTip | tab/TabInsert/GroupTextBoxText | stub |
| SelectByTypeDialog | button | SelectByTypeDialog | tab/TabHome/GroupEditing | stub |
| SelectContents | button | SelectContents | tab/TabFormat/GroupMembership | stub |
| SelectMenu | menu | SelectMenu | tab/TabHome/GroupEditing | stub |
| SendUsingEmail | task | SendUsingEmail | backstage/TabShare | stub |
| SetLanguage | button | SetLanguage | tab/TabReviewWord/GroupLanguage | stub |
| ShadowOptionsDialog | button | ShadowOptionsDialog | tab/TabHome/GroupShapeStyles | stub |
| ShapeBehavior | button | ShapeBehavior | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapeData | button | ShapeData | tab/TabShapeSheetDesign/GroupShapeSheetEditing | stub |
| ShapeDataWindow | checkBox | ShapeDataWindow | tab/TabData/GroupDataShowHide | stub |
| ShapeDataWindowShowHide | toggleButton | ShapeDataWindowShowHide | tab/TabView/GroupViewShowHide | stub |
| ShapeEffectsMenu | menu | ShapeEffectsMenu | tab/TabHome/GroupShapeStyles | stub |
| ShapeName | button | ShapeName | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapeProtection | button | ShapeProtection | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapeReports | button | ShapeReports | tab/TabReviewWord/GroupReports | stub |
| ShapeSheetDeleteRow | button | ShapeSheetDeleteRow | tab/TabShapeSheetDesign/GroupShapeSheetRows | stub |
| ShapeSheetDeleteSection | button | ShapeSheetDeleteSection | tab/TabShapeSheetDesign/GroupShapeSheetSections | stub |
| ShapeSheetEditFormula | button | ShapeSheetEditFormula | tab/TabShapeSheetDesign/GroupShapeSheetEditing | stub |
| ShapeSheetInsertRow | button | ShapeSheetInsertRow | tab/TabShapeSheetDesign/GroupShapeSheetRows | stub |
| ShapeSheetInsertSection | button | ShapeSheetInsertSection | tab/TabShapeSheetDesign/GroupShapeSheetSections | stub |
| ShapeSheetShapeAction | button | ShapeSheetShapeAction | tab/TabShapeSheetDesign/GroupShapeSheetEditing | stub |
| ShapeSheetShowFormulas | toggleButton | ShapeSheetShowFormulas | tab/TabShapeSheetDesign/GroupShapeSheetView | stub |
| ShapeSheetShowSections | button | ShapeSheetShowSections | tab/TabShapeSheetDesign/GroupShapeSheetView | stub |
| ShapeSheetShowValues | toggleButton | ShapeSheetShowValues | tab/TabShapeSheetDesign/GroupShapeSheetView | stub |
| ShapeStylesGallery | gallery | ShapeStylesGallery | tab/TabHome/GroupShapeStyles | stub |
| ShapesObjectCombine | button | ShapesObjectCombine | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectFragment | button | ShapesObjectFragment | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectIntersect | button | ShapesObjectIntersect | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectJoin | button | ShapesObjectJoin | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectOffset | button | ShapesObjectOffset | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectSubtract | button | ShapesObjectSubtract | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectTrim | button | ShapesObjectTrim | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectUnion | button | ShapesObjectUnion | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesObjectUpdateAlignmentBox | button | ShapesObjectUpdateAlignmentBox | tab/TabDeveloper/GroupShapeDesign | stub |
| ShapesRotateGallery | gallery | ShapesRotateGallery | tab/TabHome/GroupArrange | stub |
| ShapesWindow | toggleButton | ShapesWindow | tab/TabView/GroupViewShowHide | stub |
| Share | category | Share | backstage/TabShare | stub |
| SharePointWorkflowExport | button | SharePointWorkflowExport | tab/TabProcess/GroupSharepointWorkflow | stub |
| SharePointWorkflowImport | button | SharePointWorkflowImport | tab/TabProcess/GroupSharepointWorkflow | stub |
| ShareWithPeople | task | ShareWithPeople | backstage/TabShare | stub |
| ShowCommentsMenu | splitButton | ShowCommentsMenu | tab/TabReviewWord/GroupComments | stub |
| ShowFormulaTracingWindow | toggleButton | ShowFormulaTracingWindow | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| ShowGuides | checkBox | ShowGuides | tab/TabView/GroupViewShowHide | stub |
| ShowIgnoredIssues | toggleButton | ShowIgnoredIssues | tab/TabProcess/GroupDiagramValidation | stub |
| ShowPageBreaks | checkBox | ShowPageBreaks | tab/TabView/GroupViewShowHide | stub |
| ShowShapeSheet | splitButton | ShowShapeSheet | tab/TabDeveloper/GroupShapeDesign | stub |
| ShowShapeSheetDocument | toggleButton | ShowShapeSheetDocument | tab/TabDeveloper/GroupShapeDesign | stub |
| ShowShapeSheetPage | toggleButton | ShowShapeSheetPage | tab/TabDeveloper/GroupShapeDesign | stub |
| ShowShapeSheetShape | toggleButton | ShowShapeSheetShape | tab/TabDeveloper/GroupShapeDesign | stub |
| ShowTags | toggleButton | ShowTags | tab/TabReviewWord/GroupComments | stub |
| ShowTraining | button | ShowTraining | tab/HelpTab/GroupHelpAndSupport | stub |
| SizeAndPositionWindow | toggleButton | SizeAndPositionWindow | tab/TabView/GroupViewShowHide | stub |
| SkyDriveUpsellGroup | group | SkyDriveUpsellGroup | backstage/TabSave | stub |
| SlabCloudSaveAsAccessibility | group | SlabCloudSaveAsAccessibility | backstage/TabSave | stub |
| SlabCreatePPT | group | SlabCreatePPT | backstage/TabPublish | stub |
| SlabCreateWord | group | SlabCreateWord | backstage/TabPublish | stub |
| SlabLocalSaveAsAccessibility | group | SlabLocalSaveAsAccessibility | backstage/TabSave | stub |
| SlabSaveAsAccessibility | group | SlabSaveAsAccessibility | backstage/TabSave | stub |
| SmartSpace | button | SmartSpace | tab/TabData/GroupDataGraphics | stub |
| SnapAndGlueDialog | buttonDialogBoxLauncher | SnapAndGlueDialog | tab/TabView/GroupVisualAids | stub |
| SoftEdgesMoreOptions | button | SoftEdgesMoreOptions | tab/TabHome/GroupShapeStyles | stub |
| Spelling | button | Spelling | qat | stub |
| StencilClose | button | StencilClose | tab/TabStencil/GroupStencilClose | stub |
| StencilExplorer | toggleButton | StencilExplorer | tab/TabStencil/GroupStencilView | stub |
| StopIgnoringRule | button | StopIgnoringRule | tab/TabProcess/GroupDiagramValidation | stub |
| StopIgnoringThisIssue | button | StopIgnoringThisIssue | tab/TabProcess/GroupDiagramValidation | stub |
| Strikethrough | toggleButton | Strikethrough | tab/TabHome/GroupFont | stub |
| StyleExplorer | checkBox | StyleExplorer | tab/TabShapeSheetDesign/GroupShapeSheetView | stub |
| SubprocessBrowseToOtherDocument | button | SubprocessBrowseToOtherDocument | tab/TabProcess/GroupSubProcess | stub |
| SubprocessCreateFromSelection | button | SubprocessCreateFromSelection | tab/TabProcess/GroupSubProcess | stub |
| SubprocessCreateNew | button | SubprocessCreateNew | tab/TabProcess/GroupSubProcess | stub |
| SubprocessEditLink | button | SubprocessEditLink | tab/TabProcess/GroupSubProcess | stub |
| SubprocessLinkToExisting | gallery | SubprocessLinkToExisting | tab/TabProcess/GroupSubProcess | stub |
| SymbolInsertGallery | gallery | SymbolInsertGallery | tab/TabInsert/GroupTextBoxText | stub |
| SymbolsDialog | button | SymbolsDialog | tab/TabInsert/GroupTextBoxText | stub |
| TabDVDesign | tab | TabDVDesign | tab/TabDVDesign | stub |
| TabData | tab | TabData | tab/TabData | stub |
| TabDesignVisio | tab | TabDesignVisio | tab/TabDesignVisio | stub |
| TabDeveloper | tab | TabDeveloper | tab/TabDeveloper | stub |
| TabDrawInk | tab | TabDrawInk | tab/TabDrawInk | stub |
| TabFormat | tab | TabFormat | tab/TabFormat | stub |
| TabHelp | tab | TabHelp | backstage/TabHelp | stub |
| TabHome | tab | TabHome | tab/TabHome | stub |
| TabIconEditor | tab | TabIconEditor | tab/TabIconEditor | stub |
| TabInfo | tab | TabInfo | backstage/TabInfo | stub |
| TabInkToolsPens | tab | TabInkToolsPens | tab/TabInkToolsPens | stub |
| TabInsert | tab | TabInsert | tab/TabInsert | stub |
| TabOfficeFeedback | tab | TabOfficeFeedback | backstage/TabOfficeFeedback | stub |
| TabOfficeStart | tab | TabOfficeStart | backstage/TabOfficeStart | stub |
| TabOrgChart | tab | TabOrgChart | tab/TabOrgChart | stub |
| TabPictureToolsFormat | tab | TabPictureToolsFormat | tab/TabPictureToolsFormat | stub |
| TabPrint | tab | TabPrint | backstage/TabPrint | stub |
| TabProcess | tab | TabProcess | tab/TabProcess | stub |
| TabPublish | tab | TabPublish | backstage/TabPublish | stub |
| TabRecent | tab | TabRecent | backstage/TabRecent | stub |
| TabReviewWord | tab | TabReviewWord | tab/TabReviewWord | stub |
| TabSave | control | TabSave | backstage/TabSave | stub |
| TabShapeSheetDesign | tab | TabShapeSheetDesign | tab/TabShapeSheetDesign | stub |
| TabShare | tab | TabShare | backstage/TabShare | stub |
| TabStencil | tab | TabStencil | tab/TabStencil | stub |
| TabView | tab | TabView | tab/TabView | stub |
| TaskPanesMenuVisio | menu | TaskPanesMenuVisio | tab/TabView/GroupViewShowHide | stub |
| TextAlignBottom | toggleButton | TextAlignBottom | tab/TabHome/GroupParagraph | stub |
| TextAlignMiddle | toggleButton | TextAlignMiddle | tab/TabHome/GroupParagraph | stub |
| TextAlignTop | toggleButton | TextAlignTop | tab/TabHome/GroupParagraph | stub |
| TextAllCaps | toggleButton | TextAllCaps | tab/TabHome/GroupFont | stub |
| TextBlockTool | toggleButton | TextBlockTool | tab/TabHome/GroupTools | stub |
| TextBoxInsertHorizontal | toggleButton | TextBoxInsertHorizontal | tab/TabInsert/GroupTextBoxText | stub |
| TextBoxInsertMenu | splitButton | TextBoxInsertMenu | tab/TabInsert/GroupTextBoxText | stub |
| TextBoxInsertVertical | toggleButton | TextBoxInsertVertical | tab/TabInsert/GroupTextBoxText | stub |
| TextDirectionLeftToRight | toggleButton | TextDirectionLeftToRight | tab/TabHome/GroupParagraph | stub |
| TextDirectionRightToLeft | toggleButton | TextDirectionRightToLeft | tab/TabHome/GroupParagraph | stub |
| TextDirectionVertical | button | TextDirectionVertical | tab/TabHome/GroupParagraph | stub |
| TextInitialCaps | toggleButton | TextInitialCaps | tab/TabHome/GroupFont | stub |
| TextNormalCase | toggleButton | TextNormalCase | tab/TabHome/GroupFont | stub |
| TextRotate90Degrees | button | TextRotate90Degrees | tab/TabHome/GroupParagraph | stub |
| TextSmallCaps | toggleButton | TextSmallCaps | tab/TabHome/GroupFont | stub |
| ThemeApplyToNewShapes | toggleButton | ThemeApplyToNewShapes | tab/TabDesignVisio/GroupThemesVisio | stub |
| ThemeColorsCreateNewVisio | button | ThemeColorsCreateNewVisio | tab/TabDesignVisio/GroupThemeVariants | stub |
| ThemeColorsGalleryVisio | gallery | ThemeColorsGalleryVisio | tab/TabDesignVisio/GroupThemeVariants | stub |
| ThemeConnectorsGallery | gallery | ThemeConnectorsGallery | tab/TabDesignVisio/GroupThemeVariants | stub |
| ThemeEffectsGalleryVisio | gallery | ThemeEffectsGalleryVisio | tab/TabDesignVisio/GroupThemeVariants | stub |
| ThemeVariantsGallery | gallery | ThemeVariantsGallery | tab/TabDesignVisio/GroupThemeVariants | stub |
| ThemesGalleryVisio | gallery | ThemesGalleryVisio | tab/TabDesignVisio/GroupThemesVisio | stub |
| ThemesGalleryVisioLegacy | gallery | ThemesGalleryVisioLegacy | tab/TabDesignVisio/GroupThemesVisioLegacy | stub |
| Theming | group | Theming | backstage/TabHelp | stub |
| Thesaurus | button | Thesaurus | tab/TabReviewWord/GroupProofing | stub |
| ToggleVisioFlowPlaceholders | toggleButton | ToggleVisioFlowPlaceholders | tab/TabProcess/GroupFlow | stub |
| TraceBack | button | TraceBack | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| TraceDependentCells | toggleButton | TraceDependentCells | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| TraceForward | button | TraceForward | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| TracePrecedentCells | toggleButton | TracePrecedentCells | tab/TabShapeSheetDesign/GroupShapeSheetFormulaTracing | stub |
| Translate | button | Translate | tab/TabReviewWord/GroupLanguage | stub |
| Underline | toggleButton | Underline | tab/TabHome/GroupFont | stub |
| UpdateChannel | button | UpdateChannel | backstage/TabHelp | stub |
| UpdateIcon | button | UpdateIcon | tab/TabStencil/GroupStencilMasterShape | stub |
| UpgradeDocument | button | UpgradeDocument | backstage/TabInfo | stub |
| UsagePermissions | button | UsagePermissions | backstage/TabInfo | stub |
| ViewDirectionLeftToRight | toggleButton | ViewDirectionLeftToRight | tab/TabView/GroupViewDirection | stub |
| ViewDirectionMenu | menu | ViewDirectionMenu | tab/TabView/GroupViewDirection | stub |
| ViewDirectionRightToLeft | toggleButton | ViewDirectionRightToLeft | tab/TabView/GroupViewDirection | stub |
| ViewFullScreenView | toggleButton | ViewFullScreenView | tab/TabView/GroupDocumentViewsVisio | stub |
| ViewVisualBasicCode | button | ViewVisualBasicCode | tab/TabDeveloper/GroupControls | stub |
| VisioToPowerPoint | toggleButton | VisioToPowerPoint | tab/TabView/GroupVisioExport | stub |
| VisioToWord | toggleButton | VisioToWord | tab/TabProcess/GroupVisioExport | stub |
| VisualBasic | button | VisualBasic | tab/TabDeveloper/GroupCode | stub |
| WhatsNewRecentUpdates | button | WhatsNewRecentUpdates | tab/HelpTab/GroupHelpAndSupport | stub |
| WindowMoreWindowsDialog | toggleButton | WindowMoreWindowsDialog | tab/TabStencil/GroupStencilWindow | stub |
| WindowNew | button | WindowNew | tab/TabView/GroupWindow | stub |
| WindowSwitchWindowsMenuWord | menu | WindowSwitchWindowsMenuWord | tab/TabStencil/GroupStencilWindow | stub |
| WindowsArrangeAll | button | WindowsArrangeAll | tab/TabView/GroupWindow | stub |
| WindowsCascade | button | WindowsCascade | tab/TabView/GroupWindow | stub |
| WritebackVisioDiagramToExcelSource | button | WritebackVisioDiagramToExcelSource | tab/TabDVDesign/GroupRefreshUpdate | stub |
| ZoomDialog | button | ZoomDialog | tab/TabView/GroupZoom | stub |
| ZoomPageWidthVisio | button | ZoomPageWidthVisio | tab/TabView/GroupZoom | stub |
| _3DBevelOptionsDialog | button | _3DBevelOptionsDialog | tab/TabHome/GroupShapeStyles | stub |
| _3DRotationGallery | gallery | _3DRotationGallery | tab/TabHome/GroupShapeStyles | stub |
| _3DRotationOptionsDialog | button | _3DRotationOptionsDialog | tab/TabHome/GroupShapeStyles | stub |
| ComAddInsDialog | button | ComAddInsDialog | tab/TabDeveloper/GroupAddins | out-of-scope |
| DataLinkDataToShapes | button | DataLinkDataToShapes | tab/TabData/GroupExternalData | out-of-scope |
| GroupAddInsCustomToolbars | group | GroupAddInsCustomToolbars | tab/TabAddIns/GroupAddInsCustomToolbars | out-of-scope |
| GroupAddInsMenuCommands | group | GroupAddInsMenuCommands | tab/TabAddIns/GroupAddInsMenuCommands | out-of-scope |
| GroupAddInsToolbarCommands | group | GroupAddInsToolbarCommands | tab/TabAddIns/GroupAddInsToolbarCommands | out-of-scope |
| GroupAddins | group | GroupAddins | tab/TabDeveloper/GroupAddins | out-of-scope |
| GroupAdvancedDataLinking | group | GroupAdvancedDataLinking | tab/TabData/GroupAdvancedDataLinking | out-of-scope |
| GroupMacros | group | GroupMacros | tab/TabView/GroupMacros | out-of-scope |
| MacroPlay | button | MacroPlay | tab/TabDeveloper/GroupCode | out-of-scope |
| MacroRecord | button | MacroRecord | tab/TabDeveloper/GroupCode | out-of-scope |
| MacroRecorderPause | button | MacroRecorderPause | tab/TabDeveloper/GroupCode | out-of-scope |
| MacroSecurity | button | MacroSecurity | tab/TabDeveloper/GroupCode | out-of-scope |
| QuickDataLinkDataToShapes | button | QuickDataLinkDataToShapes | tab/TabData/GroupExternalData | out-of-scope |
| TabAddIns | tab | TabAddIns | tab/TabAddIns | out-of-scope |

# Parity matrix: registry commands

Every command id in `larrygram_commands::builtins::registry()`, including ids with no Fluent control, with a status of `implemented`, `stub`, or `out-of-scope`. Sorted by `(status, id)`; the status order is `implemented`, `stub`, `out-of-scope`.

The status is derived, never hand-edited (ADR 0037):

- `out-of-scope` when the command id is listed in `ui-spec/out-of-scope.txt` (matched case-insensitively, exactly or by a `*` glob), or every one of its Fluent ids is listed (the controls it serves are out of the alpha);
- `implemented` when `larrygram_commands::builtins::registry()` reports `CommandStatus::Implemented` for the id;
- `stub` otherwise.

An `out-of-scope` row stays out of scope even when a handler resolves: the same precedence as the control table. Regenerate with `cargo xtask parity`; the gate's `parity` step runs `cargo xtask parity --check` and fails when this file is stale.

| command | label | status |
|---------|-------|--------|
| app.office.center.options | Options | implemented |
| app.quit | Exit | implemented |
| arrange.align.bottom | Bottom | implemented |
| arrange.align.center_x | Align Center | implemented |
| arrange.align.left | Align Left | implemented |
| arrange.align.middle | Middle | implemented |
| arrange.align.right | Align Right | implemented |
| arrange.align.top | Top | implemented |
| arrange.bring_forward | Bring Forward | implemented |
| arrange.bring_to_front | Bring to Front | implemented |
| arrange.distribute.horizontal | Distribute Horizontally | implemented |
| arrange.distribute.vertical | Distribute Vertically | implemented |
| arrange.group | Group | implemented |
| arrange.send_backward | Send Backward | implemented |
| arrange.send_to_back | Send to Back | implemented |
| arrange.ungroup | Ungroup | implemented |
| connector.draw | Draw Connector | implemented |
| connector.glue_begin | Glue Begin | implemented |
| connector.glue_end | Glue End | implemented |
| connector.reroute_all | Reroute All | implemented |
| connector.style | Connector Style | implemented |
| connector.unglue | Unglue | implemented |
| data.set.property | Value | implemented |
| edit.copy | Copy | implemented |
| edit.cut | Cut | implemented |
| edit.delete | Delete | implemented |
| edit.deselect | Deselect | implemented |
| edit.duplicate | Duplicate | implemented |
| edit.paste | Paste | implemented |
| edit.redo | Redo | implemented |
| edit.select | Select | implemented |
| edit.select_all | Select All | implemented |
| edit.undo | Undo | implemented |
| file.close | Close | implemented |
| file.exit | Exit | implemented |
| file.export | Export | implemented |
| file.inspect | Inspect | implemented |
| file.new | New | implemented |
| file.open | Open | implemented |
| file.render | Render | implemented |
| file.save | Save | implemented |
| file.save_as | Save As | implemented |
| help.about | About Larrygram | implemented |
| help.welcome | Welcome | implemented |
| insert.drawing.ellipse | Ellipse | implemented |
| insert.drawing.line | Line | implemented |
| insert.drawing.rectangle | Rectangle | implemented |
| insert.drawing.text_box | Text Box | implemented |
| insert.shape | Shape | implemented |
| page.add | New Page | implemented |
| page.delete | Delete Page | implemented |
| page.move_left | Move Page Left | implemented |
| page.move_right | Move Page Right | implemented |
| page.next | Next Page | implemented |
| page.previous | Previous Page | implemented |
| page.rename | Rename Page | implemented |
| page.reorder | Reorder | implemented |
| shape.ellipse | Ellipse | implemented |
| shape.enter_group | Enter Group | implemented |
| shape.exit_group | Exit Group | implemented |
| shape.line | Line | implemented |
| shape.move | Move | implemented |
| shape.rectangle | Rectangle | implemented |
| shape.resize | Resize | implemented |
| shape.rotate | Rotate | implemented |
| shape.set.angle | Angle | implemented |
| shape.set.height | Height | implemented |
| shape.set.pin | Position | implemented |
| shape.set.text | Shape Text | implemented |
| shape.set.width | Width | implemented |
| text.begin | Edit Text | implemented |
| text.cancel | Cancel Text | implemented |
| text.commit | Commit Text | implemented |
| tools.connector | Connector Tool | implemented |
| tools.ellipse | Ellipse | implemented |
| tools.pointer | Pointer Tool | implemented |
| tools.rectangle | Rectangle | implemented |
| tools.text | Text Tool | implemented |
| view.rulers | Rulers | implemented |
| view.toggle_grid | Toggle Grid | implemented |
| view.ui_layout.panel_2016 | Panel (2016 data) | implemented |
| view.ui_layout.panel_365 | Panel (365 data) | implemented |
| view.ui_layout.panel_classic | Panel Classic Layout | implemented |
| view.ui_layout.panel_modern | Panel Modern Layout | implemented |
| view.units | Units | implemented |
| view.zoom.fit | Fit to Window | implemented |
| view.zoom.in | Zoom In | implemented |
| view.zoom.out | Zoom Out | implemented |
| app.bar.auto_height | Auto Height | stub |
| app.bar.auto_hide | Auto Hide | stub |
| app.bar.hide | Hide | stub |
| app.bar.toggle_float | Toggle Float | stub |
| app.coauth.merging | Merging | stub |
| app.guard.trust.promotion | Guard Trust Promotion | stub |
| app.maximize | Maximize | stub |
| app.mdi.maximize | Maximize | stub |
| app.mdi.minimize | Minimize | stub |
| app.mdi.restore | Restore | stub |
| app.minimize | Minimize | stub |
| app.minimize.command_bar.toggle | Toggle | stub |
| app.office.diagnostics | Diagnostics | stub |
| app.office.feedback.help | Office Feedback Help | stub |
| app.office.feedback.icon | Office Feedback Icon | stub |
| app.restore | Restore | stub |
| app.trust.center.dlg | Dlg | stub |
| arrange.align.box | Align | stub |
| arrange.align.center | Align Center | stub |
| arrange.align_objects | Align Shapes | stub |
| arrange.auto.align | Align | stub |
| arrange.auto.align.and.space | Space | stub |
| arrange.auto.generate.data.graphics | Graphics | stub |
| arrange.auto.space | Space | stub |
| arrange.diagram.check | Diagram Check | stub |
| arrange.diagram.flip.horizontal | Horizontal | stub |
| arrange.diagram.flip.vertical | Vertical | stub |
| arrange.diagram.gallery | Gallery | stub |
| arrange.diagram.navigation | Diagram Navigation | stub |
| arrange.disband.container | Container Disband | stub |
| arrange.distribute.bottom | Bottom | stub |
| arrange.distribute.center | Center | stub |
| arrange.distribute.h.space | Space | stub |
| arrange.distribute.left | Left | stub |
| arrange.distribute.middle | Middle | stub |
| arrange.distribute.right | Right | stub |
| arrange.distribute.top | Top | stub |
| arrange.distribute.v.space | Space | stub |
| arrange.distribute_shapes.distribute_center | Distribute Center | stub |
| arrange.distribute_shapes.distribute_horizontal_spacing | Distribute Horizontal Spacing | stub |
| arrange.distribute_shapes.distribute_middle | Distribute Middle | stub |
| arrange.distribute_shapes.distribute_vertical_spacing | Distribute Vertical Spacing | stub |
| arrange.flip_horizontal | Flip Horizontal | stub |
| arrange.flip_vertical | Flip Vertical | stub |
| arrange.group.office.insider.about | Group Office Insider About | stub |
| arrange.group.office.insider.channel | Group Office Insider Channel | stub |
| arrange.group.office.insider.help | Group Office Insider Help | stub |
| arrange.group.office.insider.registration | Group Office Insider Registration | stub |
| arrange.intersect | Shapes Object Intersect | stub |
| arrange.join | Shapes Object Join | stub |
| arrange.layout.apply.to.shape.style | Layout Apply To Shape Style | stub |
| arrange.layout.dynamic | Dynamic | stub |
| arrange.layout.more.options | Layout More Options | stub |
| arrange.nudge | Move Shape | stub |
| arrange.object.add.to.group | Group | stub |
| arrange.object.combine | Shapes Object Combine | stub |
| arrange.object.connect.objects | Objects | stub |
| arrange.object.convert.to.group | Group | stub |
| arrange.object.distribute.dlg | Dlg | stub |
| arrange.object.effect.glow | Object Effect Glow | stub |
| arrange.object.effect.shadow | Object Effect Shadow | stub |
| arrange.object.effect.soft.edges | Object Effect Soft Edges | stub |
| arrange.object.fragment | Shapes Object Fragment | stub |
| arrange.object.help | Help | stub |
| arrange.object.info.dlg | Dlg | stub |
| arrange.object.remove.from.group | Group | stub |
| arrange.object.reverse | Reverse | stub |
| arrange.object.rotate90 | Rotate90 | stub |
| arrange.object.swap.ends | Ends | stub |
| arrange.object.union | Shapes Object Union | stub |
| arrange.offset.dlg | Dlg | stub |
| arrange.position | Position | stub |
| arrange.relayout.shapes | Shapes | stub |
| arrange.reorder.list | List | stub |
| arrange.rotate.object | Object Rotate | stub |
| arrange.rotate90.clockwise | Clockwise | stub |
| arrange.rotate_left | Rotate Left | stub |
| arrange.rotate_right | Rotate Right | stub |
| arrange.size.and.position.window | Size And Position Window | stub |
| arrange.size.objects | Objects | stub |
| arrange.size.pos | Pos | stub |
| arrange.size.text.block | Block | stub |
| arrange.size1.d | D | stub |
| arrange.size2.d | D | stub |
| arrange.smart.align | Align | stub |
| arrange.smart.align.and.space | Space | stub |
| arrange.smart.space | Smart Space | stub |
| arrange.space.shapes.avoid.page.breaks.toggle | Toggle | stub |
| arrange.subtract | Shapes Object Subtract | stub |
| arrange.trim | Shapes Object Trim | stub |
| connector.add_line_jumps_to | Add Line Jumps To | stub |
| connector.add_line_jumps_to.first_displayed_line | First Displayed Line | stub |
| connector.add_line_jumps_to.horizontal_lines | Horizontal Lines | stub |
| connector.add_line_jumps_to.last_displayed_line | Last Displayed Line | stub |
| connector.add_line_jumps_to.last_routed_line | Last Routed Line | stub |
| connector.add_line_jumps_to.no_lines | No Lines | stub |
| connector.add_line_jumps_to.vertical_lines | Vertical Lines | stub |
| connector.auto_connect | AutoConnect | stub |
| connector.conn.points | Points | stub |
| connector.curved | Curved Connector | stub |
| connector.curved.lines | Curved Lines | stub |
| connector.dyn.conn.reroute | Reroute | stub |
| connector.dynamic.grid | Dynamic Grid | stub |
| connector.line_jump_style | Line Jump Style | stub |
| connector.line_jump_style.arc | Arc | stub |
| connector.line_jump_style.gap | Gap | stub |
| connector.line_jump_style.page_default | Page Default | stub |
| connector.line_jump_style.sides_2 | 2 Sides | stub |
| connector.line_jump_style.sides_3 | 3 Sides | stub |
| connector.line_jump_style.sides_4 | 4 Sides | stub |
| connector.line_jump_style.sides_5 | 5 Sides | stub |
| connector.line_jump_style.sides_6 | 6 Sides | stub |
| connector.line_jump_style.sides_7 | 7 Sides | stub |
| connector.line_jump_style.square | Square | stub |
| connector.right_angle | Right-Angle Connector | stub |
| connector.show.line.jumps | Show Line Jumps | stub |
| connector.straight | Straight Connector | stub |
| connector.straight.lines | Straight Lines | stub |
| data.auto.connect | Connect | stub |
| data.auto.link | Link | stub |
| data.auto.link.wiz | Wiz | stub |
| data.automatically.link | Automatically Link | stub |
| data.column.settings.dlg | Dlg | stub |
| data.explorer.window | Window | stub |
| data.external.add | Add... | stub |
| data.external.arrange_by | Arrange by | stub |
| data.external.column_settings | Column Settings... | stub |
| data.external.configure_refresh | Configure Refresh... | stub |
| data.external.data_source.add | Add... | stub |
| data.external.data_source.properties | Properties... | stub |
| data.external.data_source.remove | Remove | stub |
| data.external.data_source.rename | Rename | stub |
| data.external.link_to_selected_shapes | Link to Selected Shapes | stub |
| data.external.linked_shapes | Linked Shapes | stub |
| data.external.properties | Properties... | stub |
| data.external.remove | Remove | stub |
| data.external.rename | Rename | stub |
| data.external.unlink | Unlink | stub |
| data.graphic.callouts | Graphic Callouts | stub |
| data.graphic.create.new | Graphic Create New | stub |
| data.graphic.edit | Graphic Edit | stub |
| data.graphic.position | Graphic Position | stub |
| data.graphic.position.horizontal | Graphic Position Horizontal | stub |
| data.graphic.position.vertical | Graphic Position Vertical | stub |
| data.graphics.configure | Graphics Configure | stub |
| data.graphics.gallery.visio | Graphics Gallery Visio | stub |
| data.insert_data_solutions.calendar | Calendar... | stub |
| data.insert_data_solutions.gantt_chart | Gantt Chart... | stub |
| data.insert_data_solutions.organization_chart | Organization Chart... | stub |
| data.insert_data_solutions.timeline | Timeline... | stub |
| data.insert_pivotdiagram | Insert PivotDiagram... | stub |
| data.link.row.to.shape | Shape | stub |
| data.options | Options | stub |
| data.recordset.properties | Properties | stub |
| data.recordset.set.command | Command | stub |
| data.recordset.set.primary.key | Key | stub |
| data.refresh | Refresh Data | stub |
| data.refresh.add.conflict | Conflict | stub |
| data.refresh.config.dlg | Dlg | stub |
| data.refresh.delete.conflict | Conflict | stub |
| data.refresh.dlg | Dlg | stub |
| data.refresh.resolve.conflict | Conflict | stub |
| data.refresh_all | Refresh All | stub |
| data.selector.dlg | Dlg | stub |
| data.unlink.row | Row | stub |
| data.unlink.shape | Shape Data | stub |
| data.update.channel | Update Channel | stub |
| data.update.columns.in.linked.shapes | Shapes | stub |
| data.update.content.cache | Cache | stub |
| data.update.icon | Update Icon | stub |
| developer.prog.ref.help | Help | stub |
| developer.run.add.on.dlg | Dlg | stub |
| developer.run.addon.menu | Menu | stub |
| developer.run_add_on | Run Add-On... | stub |
| edit.action | Action... | stub |
| edit.break.ole.link | Link | stub |
| edit.cancel.in.place.editing | Editing | stub |
| edit.clear | Delete | stub |
| edit.connection_point.inward | Inward | stub |
| edit.connection_point.inward_and_outward | Inward & Outward | stub |
| edit.connection_point.outward | Outward | stub |
| edit.convert.object | Object | stub |
| edit.create_shortcut | Create Shortcut | stub |
| edit.del.connect.pt | Pt | stub |
| edit.delete.comment | Comment | stub |
| edit.delete.connectors | Connectors | stub |
| edit.delete.data.graphic | Graphic | stub |
| edit.delete.data.recordset | Recordset | stub |
| edit.delete.theme | Theme | stub |
| edit.delete_back_word | Delete Word Back | stub |
| edit.delete_forward_word | Delete Forward Word | stub |
| edit.delete_or_inherit_section | Delete or Inherit Section | stub |
| edit.deselect.all | All | stub |
| edit.drag.duplicate | Duplicate | stub |
| edit.duplicate.data.graphic | Graphic | stub |
| edit.duplicate.page | Duplicate This Page | stub |
| edit.duplicate.theme | Theme | stub |
| edit.find | Find | stub |
| edit.insert.field | Field Insert | stub |
| edit.insert.object | Object | stub |
| edit.links | Links | stub |
| edit.mod.connect.pt | Pt | stub |
| edit.mod.control.pt | Pt | stub |
| edit.mov.connect.pt | Pt | stub |
| edit.move.comment | Comment | stub |
| edit.move1.d | D | stub |
| edit.move2.d | D | stub |
| edit.open.object | Object | stub |
| edit.paste.duplicate | Paste Duplicate | stub |
| edit.paste.link | Link | stub |
| edit.paste.shortcut | Shortcut | stub |
| edit.paste.special | Special | stub |
| edit.paste.special.dialog | Paste Special Dialog | stub |
| edit.paste.to.location | Location | stub |
| edit.paste_shortcut | Paste Shortcut | stub |
| edit.recalc.object.wh | Wh | stub |
| edit.reconvert | Reconvert | stub |
| edit.redo.multiple | Multiple | stub |
| edit.redo.or.repeat | Redo Or Repeat | stub |
| edit.reinstate.validation.issue | Issue | stub |
| edit.remove.data.graphic.from.sel | Sel | stub |
| edit.remove.from.all.containers | Containers | stub |
| edit.remove.member.from.container | Container | stub |
| edit.remove.member.from.list | List | stub |
| edit.remove.theme.from.sel | Remove Theme | stub |
| edit.remove.vba.from.active.doc | Doc | stub |
| edit.repeat | Repeat | stub |
| edit.replace | Replace | stub |
| edit.replace.dialog | Replace Dialog | stub |
| edit.replace.shape | Replace Shape | stub |
| edit.right.button.icon.color | Right Button Icon Color | stub |
| edit.right.button.icon.transparent | Right Button Icon Transparent | stub |
| edit.right.drag.cancel | Cancel | stub |
| edit.right.drag.copy | Copy | stub |
| edit.right.drag.link | Link | stub |
| edit.right.drag.move | Move | stub |
| edit.select.by.type.dialog | Select By Type Dialog | stub |
| edit.select.container.members | Members | stub |
| edit.select.contents | Select Contents | stub |
| edit.select.special | Select | stub |
| edit.sw.accept | Accept | stub |
| edit.sw.add.section.dlg | Dlg | stub |
| edit.sw.cancel | Cancel | stub |
| edit.sw.change.row.type.dlg | Dlg | stub |
| edit.sw.delete.row | Row | stub |
| edit.sw.delete.section | Section | stub |
| edit.sw.expand.row | Row | stub |
| edit.sw.formula | Formula | stub |
| edit.sw.insert.row | Row | stub |
| edit.sw.insert.row.after | After | stub |
| edit.sw.paste.function.dlg | Dlg | stub |
| edit.sw.paste.name.dlg | Dlg | stub |
| edit.sw.shape.action.dlg | Dlg | stub |
| edit.sw.show.formulas | Formulas | stub |
| edit.sw.show.sections.dlg | Show Sections | stub |
| edit.sw.show.toggle | Toggle | stub |
| edit.sw.show.values | Values | stub |
| edit.theme | Theme | stub |
| edit.theme.colors | Colors | stub |
| edit.theme.effects | Effects | stub |
| edit.undo.multiple | Multiple | stub |
| file.browse.sample.drawings | Drawings | stub |
| file.checkin | Checkin | stub |
| file.checkout | Checkout | stub |
| file.choose.templates | Templates | stub |
| file.close.ink.tools.tab | Ink Tools Close | stub |
| file.close_window | Close Window | stub |
| file.create.edit.master | Edit Master | stub |
| file.create.new.drawing | Drawing | stub |
| file.create.shortcut | Shortcut | stub |
| file.create.stage.view.diagram | Create Stage View Diagram | stub |
| file.create.workflow.diagram | Create Workflow Diagram | stub |
| file.email.routing | Routing | stub |
| file.export.database.addon | Addon | stub |
| file.header.footer | Footer | stub |
| file.import | Import | stub |
| file.last.file1 | File1 | stub |
| file.last.file10 | File10 | stub |
| file.last.file11 | File11 | stub |
| file.last.file12 | File12 | stub |
| file.last.file13 | File13 | stub |
| file.last.file14 | File14 | stub |
| file.last.file15 | File15 | stub |
| file.last.file16 | File16 | stub |
| file.last.file17 | File17 | stub |
| file.last.file18 | File18 | stub |
| file.last.file19 | File19 | stub |
| file.last.file2 | File2 | stub |
| file.last.file20 | File20 | stub |
| file.last.file3 | File3 | stub |
| file.last.file4 | File4 | stub |
| file.last.file5 | File5 | stub |
| file.last.file6 | File6 | stub |
| file.last.file7 | File7 | stub |
| file.last.file8 | File8 | stub |
| file.last.file9 | File9 | stub |
| file.new.blank.drawing | Drawing | stub |
| file.new.blank.drawing.metric | Metric | stub |
| file.new.blank.drawing.us | Us | stub |
| file.new.blank.stencil | New Blank Stencil | stub |
| file.new.blank.stencil.metric | New Blank Stencil Metric | stub |
| file.new.blank.stencil.us | New Blank Stencil Us | stub |
| file.new.def.doc.blank.drawing | Drawing | stub |
| file.new.from.existing | Existing | stub |
| file.new.stencil.dlg | Dlg | stub |
| file.new.theme.colors | Theme Colors Create New Visio | stub |
| file.new.theme.effects | Effects | stub |
| file.open.active.object | Object | stub |
| file.open.comment.for.edit | Edit | stub |
| file.open.in.visio | Visio | stub |
| file.open.stencil | Open Stencil | stub |
| file.print | Print | stub |
| file.print.page | Page | stub |
| file.print.shape.sheet | Print Shape Sheet | stub |
| file.print_preview | Print Preview | stub |
| file.publish.to.process.repository | Repository | stub |
| file.publish.to.visio.services | Services | stub |
| file.save.as.drawing | Drawing | stub |
| file.save.as.drawing.previous.file.format | Format | stub |
| file.save.as.dwg | Dwg | stub |
| file.save.as.emf | Emf | stub |
| file.save.as.fixed.format.dlg | Dlg | stub |
| file.save.as.jpg | Jpg | stub |
| file.save.as.png | Png | stub |
| file.save.as.svg | Svg | stub |
| file.save.as.template | Template | stub |
| file.save.as.web.page | Page | stub |
| file.save.for.auto.recover | Recover | stub |
| file.save.workspace | Workspace | stub |
| file.send.as.attachment | Send As Attachment | stub |
| file.send.as.mail | Mail | stub |
| file.shapes.organize_my_shapes | Organize My Shapes | stub |
| file.summary.info.dlg | Dlg | stub |
| file.undo.checkout | Checkout | stub |
| file.upgrade.document | Upgrade Document | stub |
| file.upgrade.theme.model | Model | stub |
| format.all.text.props | Props | stub |
| format.apply.data.graphic | Data Graphic Apply After Linking Data | stub |
| format.apply.data.graphic.after.link | Link | stub |
| format.apply.main.theme | Theme | stub |
| format.apply.main.theme.to.document | Document | stub |
| format.apply.main.theme.to.page | Page | stub |
| format.apply.theme.colors | Colors | stub |
| format.apply.theme.effects | Effects | stub |
| format.apply.theme.to.doc | Doc | stub |
| format.apply.theme.to.new.shapes.toggle | Theme Apply To New Shapes | stub |
| format.apply.theme.to.page | Page | stub |
| format.behavior | Behavior | stub |
| format.block | Block | stub |
| format.change_text_direction | Change Text Direction | stub |
| format.corners | Corners | stub |
| format.cust.prop.def | Def | stub |
| format.cust.prop.edit | Shape Data | stub |
| format.decrease.indent | Indent Decrease Word | stub |
| format.decrease.para.spacing | Spacing | stub |
| format.define.styles | Styles | stub |
| format.double.click | Click | stub |
| format.fill | Fill | stub |
| format.fill_color.no_fill | No Fill | stub |
| format.fill_color.use_default | Use Default | stub |
| format.increase.indent | Indent Increase Word | stub |
| format.increase.para.spacing | Spacing | stub |
| format.ink.dlg | Dlg | stub |
| format.line | Line | stub |
| format.line_color.no_line | No Line | stub |
| format.line_color.use_default | Use Default | stub |
| format.line_ends.no_line_ends | No Line Ends | stub |
| format.line_pattern.no_line | No Line | stub |
| format.paragraph | Paragraph | stub |
| format.picture.autobalance | Autobalance | stub |
| format.picture.compression.dlg | Dlg | stub |
| format.protection | Protection | stub |
| format.set.add.markup | Markup | stub |
| format.set.char.color | Color | stub |
| format.set.container.properties | Properties | stub |
| format.set.dyn.conn.appearance.curved | Curved | stub |
| format.set.dyn.conn.appearance.default | Default | stub |
| format.set.dyn.conn.appearance.straight | Straight | stub |
| format.set.dyn.conn.reroute.as.needed | Needed | stub |
| format.set.dyn.conn.reroute.freely | Freely | stub |
| format.set.dyn.conn.reroute.never | Never | stub |
| format.set.dyn.conn.reroute.on.crossover | Crossover | stub |
| format.set.dyn.conn.routing.style | Style | stub |
| format.set.fill.color | Fill Color | stub |
| format.set.fill.pattern | Pattern | stub |
| format.set.fill.shadow | Shadow | stub |
| format.set.header.footer | Footer | stub |
| format.set.index.in.stencil | Stencil | stub |
| format.set.language.dlg | Set Language | stub |
| format.set.line.color | Line Color | stub |
| format.set.line.corner.style | Style | stub |
| format.set.line.ends | Ends | stub |
| format.set.line.pattern | Line Pattern | stub |
| format.set.line.weight | Line Weight | stub |
| format.set.page.orientation | Page Orientation | stub |
| format.set.page.plow | Plow | stub |
| format.set.page.size | Size | stub |
| format.set.theme.behavior | Behavior | stub |
| format.shadow | Shadow | stub |
| format.shape.pane | Format Shape | stub |
| format.special | Special | stub |
| format.style | Style | stub |
| format.style.explorer | Style Explorer | stub |
| format.tabs | Tabs | stub |
| format.text | Text | stub |
| format.text.align_bottom | Bottom-Align Text | stub |
| format.text.align_center | Center Text | stub |
| format.text.align_justify | Justify Text | stub |
| format.text.align_left | Align Text Left | stub |
| format.text.align_middle | Center Text Vertically | stub |
| format.text.align_right | Align Text Right | stub |
| format.text.align_top | Top-Align Text | stub |
| format.text.all_caps | Text All Caps | stub |
| format.text.bold | Bold | stub |
| format.text.decrease_size | Decrease Font Size | stub |
| format.text.increase_size | Increase Font Size | stub |
| format.text.italic | Italic | stub |
| format.text.small_caps | Text Small Caps | stub |
| format.text.subscript | Subscript | stub |
| format.text.underline | Underline | stub |
| format.text_color.use_default | Use Default | stub |
| format.theme | Theme... | stub |
| format.transparency.no_transparency | No Transparency | stub |
| help.contents | Help | stub |
| help.mode | Mode | stub |
| help.research.look.up | Up | stub |
| help.research.thesaurus | Research | stub |
| help.research.translate | Translate | stub |
| help.rhi | Rhi | stub |
| help.rhi.dlg | Dlg | stub |
| help.sample_diagrams | Sample Diagrams | stub |
| help.search | Search | stub |
| help.shape.basics | Basics | stub |
| help.templates | Templates | stub |
| insert.acquire.images | Images | stub |
| insert.add.connect.pt | Pt | stub |
| insert.add.control.pt | Pt | stub |
| insert.add.data.recordset | Recordset | stub |
| insert.add.member.to.container | Container | stub |
| insert.add.ons | Add Ons | stub |
| insert.add.text.shape | Shape | stub |
| insert.add.to.all.containers | Containers | stub |
| insert.add.to.new.container | Container | stub |
| insert.auto.cad.add.on | On | stub |
| insert.check.box.control | Control | stub |
| insert.clip.art | Clip Art Insert Dialog | stub |
| insert.clip.art.dlg | Dlg | stub |
| insert.combo.box.control | Control | stub |
| insert.comment | Comment | stub |
| insert.control.dlg | Dlg | stub |
| insert.data.map | Map | stub |
| insert.drop.and.contain | Contain | stub |
| insert.drop.and.insert.into.list | List | stub |
| insert.drop.callout | Callout | stub |
| insert.drop.many.linked | Linked | stub |
| insert.drop.many.on.page | Page | stub |
| insert.drop.on.page | Page | stub |
| insert.drop.on.stencil | Stencil | stub |
| insert.drop.on.text | Text | stub |
| insert.hyper.link | Link | stub |
| insert.hyperlink.add_to_favorites | Add to Favorites | stub |
| insert.hyperlink.copy_hyperlink | Copy Hyperlink | stub |
| insert.hyperlink.edit_hyperlink | Edit Hyperlink | stub |
| insert.hyperlink.open | Open | stub |
| insert.hyperlink.open_in_new_window | Open in New Window | stub |
| insert.image.control | Control | stub |
| insert.image.properties | Properties | stub |
| insert.image.properties.dlg | Dlg | stub |
| insert.inser.text.box.control | Control | stub |
| insert.label.control | Control | stub |
| insert.legend.horizontal | Legend Insert | stub |
| insert.legend.vertical1 | Vertical1 | stub |
| insert.list.box.control | Control | stub |
| insert.m365.picture | M365 Picture | stub |
| insert.member.into.list | List | stub |
| insert.microsoft.graph | Graph | stub |
| insert.name | Name... | stub |
| insert.new.background.page | Background Page Insert | stub |
| insert.page.tab | Tab | stub |
| insert.push.button.control | Control | stub |
| insert.radio.button.control | Control | stub |
| insert.row | Row | stub |
| insert.row_after | Row After | stub |
| insert.scroll.bar.control | Control | stub |
| insert.section | Section... | stub |
| insert.spin.control | Control | stub |
| insert.stamp.tool | Tool | stub |
| insert.text.box | Text Box Insert Horizontal | stub |
| insert.toggle.button.control | Control | stub |
| insert.vert.text.box | Vertical Text Box | stub |
| insert.word.art | Art | stub |
| layer.dialog | Dialog | stub |
| layer.dlg | Dlg | stub |
| layer.properties.dialog | Properties Dialog | stub |
| layer.setup.dlg | Dlg | stub |
| page.auto.size.toggle | Auto Size Page | stub |
| page.measure.units.dlg | Dlg | stub |
| page.multiple.file.import | Import | stub |
| page.pages.list | List | stub |
| page.relayout | Relayout | stub |
| page.reorder_pages | Reorder Pages... | stub |
| page.setup.layout_and_routing | Layout & Routing... | stub |
| page.size.dlg | Size | stub |
| page.size.to.fit.drawing | Drawing Page | stub |
| review.check.compatibility | Compatibility | stub |
| review.delete_markup | Delete Markup | stub |
| review.delete_markup.delete_all_markup_overlays_in_document | Delete All Markup Overlays | stub |
| review.delete_markup.delete_markup_overlays_on_current_page.all_on_current_page | All on Current Page | stub |
| review.edit_comment | Edit Comment | stub |
| review.ignore.this.issue | Ignore This Issue | stub |
| review.ignore.validation.issue | Issue | stub |
| review.ignore.validation.rule | Ignore Rule | stub |
| review.issues.stop_ignoring_rule | Stop Ignoring Rule | stub |
| review.issues.stop_ignoring_this_issue | Stop Ignoring This Issue | stub |
| review.new.comment | New Comment | stub |
| review.reviewer.visibility.all | All | stub |
| review.reviewer.visibility.none | None | stub |
| review.reviewers.hide_all | Hide All | stub |
| review.show_hide_markup | Show/Hide Markup | stub |
| review.spelling.change | Spelling | stub |
| review.spelling.options.dlg | Dlg | stub |
| review.validate.diagram | Diagram | stub |
| review.validation.issue.navigate.to.shape | Shape | stub |
| review.validation.issues.arrange.by.category | Category | stub |
| review.validation.issues.arrange.by.ignored | Ignored | stub |
| review.validation.issues.arrange.by.page | Page | stub |
| review.validation.issues.arrange.by.rule | Rule | stub |
| review.validation.issues.arrange.original.order | Order | stub |
| review.validation.issues.window.toggle | Issues Window | stub |
| shape.actions | Actions | stub |
| shape.arc | Arc | stub |
| shape.associate.callout | Callout | stub |
| shape.behavior | Behavior | stub |
| shape.comment | Comment | stub |
| shape.comment.delete | Delete | stub |
| shape.comment.dlg | Dlg | stub |
| shape.container.auto.resize.expand.contract | Contract | stub |
| shape.container.auto.resize.expand.only | Only | stub |
| shape.container.auto.resize.off | Off | stub |
| shape.container.heading.styles | Container Heading Styles | stub |
| shape.container.margins | Container Margins | stub |
| shape.container.no.heading.toggle | Toggle | stub |
| shape.container.styles | Container Styles | stub |
| shape.convert_ink_to_geometry | Convert Ink to Geometry | stub |
| shape.convert_ink_to_text | Convert Ink to Text | stub |
| shape.crop.object | Object | stub |
| shape.data.define_shape_data | Define Shape Data... | stub |
| shape.data.edit_data_graphic | Edit Data Graphic... | stub |
| shape.data.window | Data Window | stub |
| shape.data.window.show.hide | Data Window Show Hide | stub |
| shape.draw.add.guide | Guide | stub |
| shape.draw.convert.ink.to.geometry | Draw Convert Ink To Geometry | stub |
| shape.draw.convert.visio.ink.to.text | Draw Convert Visio Ink To Text | stub |
| shape.draw.fill.style | Style | stub |
| shape.draw.glue | Glue | stub |
| shape.draw.line.style | Style | stub |
| shape.draw.oval | Oval | stub |
| shape.draw.rect | Rect | stub |
| shape.draw.region | Region | stub |
| shape.draw.select | Draw Select | stub |
| shape.draw.snap | Snap | stub |
| shape.draw.text.style | Style | stub |
| shape.draw.zoom | Zoom | stub |
| shape.drawing.explorer | Drawing Explorer | stub |
| shape.drawing.tools | Developer Drawing Tools | stub |
| shape.edit_icon_image | Edit Icon Image | stub |
| shape.edit_master_shape | Edit Master Shape | stub |
| shape.effects | Effects | stub |
| shape.explorer | Explorer | stub |
| shape.gallery.add.on | On | stub |
| shape.geo | Geo | stub |
| shape.hand | Hand | stub |
| shape.icon.bucket.tool | Icon Bucket | stub |
| shape.icon.edit.close | Icon Edit Close | stub |
| shape.icon.insert.from.file | Icon Insert From File | stub |
| shape.icon.lasso.tool | Tool | stub |
| shape.icon.left.color | Left Button Icon Color | stub |
| shape.icon.pencil.tool | Icon Pencil | stub |
| shape.icon.right.tool | Tool | stub |
| shape.icon.select.area | Icon Select Area | stub |
| shape.icon.select.lasso | Icon Select Lasso | stub |
| shape.icon.select.net | Net | stub |
| shape.insert_shape_after | Insert Shape After | stub |
| shape.insert_shape_before | Insert Shape Before | stub |
| shape.intersect | Intersect | stub |
| shape.layer.toolbar | Toolbar | stub |
| shape.lock.container | Container Lock | stub |
| shape.master.add_to_my_shapes.add_to_existing_stencil | Add To Existing Stencil... | stub |
| shape.master.add_to_my_shapes.add_to_new_stencil | Add To New Stencil... | stub |
| shape.master.add_to_quick_shapes | Add to Quick Shapes | stub |
| shape.master.delete_master | Delete Master | stub |
| shape.master.edit_master.properties | Properties... | stub |
| shape.master.explorer | Master Explorer | stub |
| shape.master.properties | Master Properties | stub |
| shape.master.remove_from_quick_shapes | Remove from Quick Shapes | stub |
| shape.master.rename_master | Rename Master | stub |
| shape.master.setup | Setup | stub |
| shape.name | Name | stub |
| shape.protection | Protection | stub |
| shape.reports | Reports | stub |
| shape.search.window.toggle | Toggle | stub |
| shape.shape.find_similar_shapes | Find Similar Shapes | stub |
| shape.shapes.object.offset | Shapes Object Offset | stub |
| shape.shapes.object.update.alignment | Shapes Object Update Alignment | stub |
| shape.shapes.rotate | Shapes Rotate | stub |
| shape.shapes.window | Shapes Window | stub |
| shape.sheet.delete.row | Sheet Delete Row | stub |
| shape.sheet.delete.section | Sheet Delete Section | stub |
| shape.sheet.edit.formula | Sheet Edit Formula | stub |
| shape.sheet.insert.row | Sheet Insert Row | stub |
| shape.sheet.insert.section | Sheet Insert Section | stub |
| shape.sheet.shape.action | Sheet Shape Action | stub |
| shape.spline | Freeform | stub |
| shape.sten.activate | Activate | stub |
| shape.sten.auto.arrange | Arrange | stub |
| shape.sten.cleanup | Cleanup | stub |
| shape.sten.close | Close | stub |
| shape.sten.drawing.explorer | Explorer | stub |
| shape.sten.edit.drawing | Drawing | stub |
| shape.sten.edit.icon | Edit Icon | stub |
| shape.sten.edit.off | Off | stub |
| shape.sten.edit.on | On | stub |
| shape.sten.edit.toggle | Toggle | stub |
| shape.sten.icon.and.detail | Detail | stub |
| shape.sten.icon.and.name | Name | stub |
| shape.sten.icon.only | Only | stub |
| shape.sten.image.master | Master | stub |
| shape.sten.name.master | Master | stub |
| shape.sten.name.only | Only | stub |
| shape.sten.names.under.icons | Names Under Icons | stub |
| shape.sten.new.master | New Master | stub |
| shape.sten.properties | Properties | stub |
| shape.sten.save | Save | stub |
| shape.sten.save.as | As | stub |
| shape.stencil.collapse_all_groups | Collapse All Groups | stub |
| shape.stencil.edit_stencil | Edit Stencil | stub |
| shape.stencil.expand_all_groups | Expand All Groups | stub |
| shape.stencil.order.move_down | Move Down | stub |
| shape.stencil.order.move_up | Move Up | stub |
| shape.stencil.properties | Properties... | stub |
| shape.stencil.reset_stencil | Reset Stencil | stub |
| shape.studio.add.on | On | stub |
| shape.styles | Styles | stub |
| shape.transparency | Transparency | stub |
| shape.transparency.dlg | Dlg | stub |
| shape.update_icon_image | Update Icon Image | stub |
| shape.vert | Vert | stub |
| text.bullets | Bullets | stub |
| text.direction.left.to.right | Direction Left To Right | stub |
| text.direction.right.to.left | Direction Right To Left | stub |
| text.direction.vertical | Direction Vertical | stub |
| text.double.strike.through | Through | stub |
| text.double.u.line | Line | stub |
| text.edit.ruler | Ruler | stub |
| text.edit.state | State | stub |
| text.font | Font | stub |
| text.font.color | Font Color | stub |
| text.font.size | Font Size | stub |
| text.font.size.decrease | Font Size Decrease | stub |
| text.font.size.increase | Font Size Increase | stub |
| text.h.align.distribute | Distribute | stub |
| text.horizontal_in_vertical | Horizontal in Vertical... | stub |
| text.initial.caps | Initial Caps | stub |
| text.normal.case | Normal Case | stub |
| text.rotate90 | Rotate90 Degrees | stub |
| text.size | Size | stub |
| text.strike.through | Through | stub |
| text.style | Style | stub |
| text.super.script | Script | stub |
| text.u.line | Line | stub |
| tools.accessibility.checker | Accessibility Checker | stub |
| tools.allow.personally.identifiable.information | Allow Personally Identifiable Information | stub |
| tools.allow.themes | Allow Themes | stub |
| tools.area.select | Area Select | stub |
| tools.array.shapes.add.on | On | stub |
| tools.arrows.more | Arrows More | stub |
| tools.attach.data.source | Attach Data Source | stub |
| tools.automatic.resize | Automatic Resize | stub |
| tools.automatic.resize.always.fit.to.contents | Automatic Resize Always Fit To Contents | stub |
| tools.automatic.resize.expand.as.needed | Automatic Resize Expand As Needed | stub |
| tools.automatic.resize.none | Automatic Resize None | stub |
| tools.background.color | Background Color | stub |
| tools.backgrounds | Backgrounds | stub |
| tools.bevel.shape | Bevel Shape | stub |
| tools.bevel_options_3d | 3 D Bevel Options Dialog | stub |
| tools.blank.page.insert | Blank Page Insert | stub |
| tools.blank.page.insert.visio | Blank Page Insert Visio | stub |
| tools.borders.and.titles | Borders And Titles | stub |
| tools.button.group.cloud.services | Button Group Cloud Services | stub |
| tools.button.group.item.save.cloud.services | Button Group Item Save Cloud Services | stub |
| tools.button.group.item.save.local.services | Button Group Item Save Local Services | stub |
| tools.button.group.local.services | Button Group Local Services | stub |
| tools.button.group.recent.documents | Button Group Recent Documents | stub |
| tools.button.groups.save | Button Groups Save | stub |
| tools.button.open.browse | Button Open Browse | stub |
| tools.cad.drawing | Cad Drawing | stub |
| tools.change.case.dialog.classic | Change Case Dialog Classic | stub |
| tools.chart.insert | Chart Insert | stub |
| tools.clear.icon.colors | Clear Icon Colors | stub |
| tools.commenting | Commenting | stub |
| tools.comments.pane | Comments Pane Split | stub |
| tools.connection | Connection Point Tool | stub |
| tools.connection.point | Connection Point | stub |
| tools.connection.points | Connection Points | stub |
| tools.contact.us | Contact Us | stub |
| tools.control.properties | Control Properties | stub |
| tools.controls | Controls | stub |
| tools.crop | Crop Tool | stub |
| tools.cust.prop | Prop | stub |
| tools.cust.prop.define | Define | stub |
| tools.custom.property.sets | Sets | stub |
| tools.design.mode | Design Mode | stub |
| tools.detect.and.repair | Repair | stub |
| tools.dlg.custom.fit | Fit | stub |
| tools.dv.export.vtpx | Dv Export Vtpx | stub |
| tools.dv.open.data.source | Dv Open Data Source | stub |
| tools.dv.relink.data.source | Dv Relink Data Source | stub |
| tools.embellishment | Embellishment | stub |
| tools.event.monitor | Event Monitor | stub |
| tools.external.data.window | External Data Window | stub |
| tools.fill.dialog | Fill Dialog | stub |
| tools.find.dialog | Find Dialog | stub |
| tools.finger.painting.mode | Finger Painting Mode | stub |
| tools.flyout.anchor.insert.pictures | Flyout Anchor Insert Pictures | stub |
| tools.format_painter | Format Painter | stub |
| tools.formula_tracing.trace_dependents | Trace Dependents | stub |
| tools.formula_tracing.trace_precedents | Trace Precedents | stub |
| tools.formula_tracing.window | Formula Tracing Window | stub |
| tools.freeform | Freeform | stub |
| tools.glows.more.options | Glows More Options | stub |
| tools.hyperlink.hier | Hier | stub |
| tools.hyperlink.insert | Hyperlink Insert | stub |
| tools.hyperlink.list | List | stub |
| tools.icons.and.details | Icons And Details | stub |
| tools.icons.and.names | Icons And Names | stub |
| tools.icons.only | Icons Only | stub |
| tools.import.flow.as.visio.bpmn.diagram | Import Flow As Visio Bpmn Diagram | stub |
| tools.inet.add.to.favorites | Favorites | stub |
| tools.inet.copy.hyperlink | Hyperlink | stub |
| tools.inet.delete.hlink | Hlink | stub |
| tools.inet.diagramming.resources | Resources | stub |
| tools.inet.edit.hyperlink | Hyperlink | stub |
| tools.inet.go.back | Back | stub |
| tools.inet.go.forward | Forward | stub |
| tools.inet.knowledge.base | Base | stub |
| tools.inet.open.h.link | Link | stub |
| tools.inet.open.h.link.new.wnd | Wnd | stub |
| tools.inet.paste.as.hyperlink | Paste As Hyperlink | stub |
| tools.inet.user.search.page | Page | stub |
| tools.inet.visio.home.page | Page | stub |
| tools.inet.visio.on.the.web | Web | stub |
| tools.inet.visio.solutions.library | Library | stub |
| tools.ink.auto.create.shapes | Ink Auto Create Shapes | stub |
| tools.ink.ballpoint.pen | Ink Ballpoint Pen | stub |
| tools.ink.close_ink_shape | Close Ink Shape | stub |
| tools.ink.color.picker | Ink Color Picker | stub |
| tools.ink.covert.to.geometry | Ink Covert To Geometry | stub |
| tools.ink.covert.to.text.visio | Ink Covert To Text Visio | stub |
| tools.ink.eraser | Ink Eraser | stub |
| tools.ink.highlighter | Ink Highlighter | stub |
| tools.ink.ink_pen_3 | Ink Pen 3 | stub |
| tools.ink.ink_pen_4 | Ink Pen 4 | stub |
| tools.ink.ink_pen_5 | Ink Pen 5 | stub |
| tools.ink.review.pen | Pen | stub |
| tools.ink.shape.close | Ink Shape Close | stub |
| tools.ink.stock.pen0 | Pen0 | stub |
| tools.ink.stock.pen1 | Pen1 | stub |
| tools.ink.tool | Ink | stub |
| tools.insights | Insights | stub |
| tools.inventory | Inventory | stub |
| tools.language.commands | Language Commands | stub |
| tools.language.preferences.dlg | Language Preferences | stub |
| tools.lasso.select.visio | Lasso Select Visio | stub |
| tools.layers | Layers | stub |
| tools.layout.shapes.dlg | Dlg | stub |
| tools.left.button.icon.transparent | Left Button Icon Transparent | stub |
| tools.legacy.mode | Legacy Mode | stub |
| tools.line.arrows | Line Arrows | stub |
| tools.line.dialog | Line Dialog | stub |
| tools.line.styles.dialog | Line Styles Dialog | stub |
| tools.line.thickness | Line Thickness | stub |
| tools.manage.privacy | Manage Privacy | stub |
| tools.more.data.graphics | More Data Graphics | stub |
| tools.more.page.sizes | More Page Sizes | stub |
| tools.mso.auto.correct | Correct | stub |
| tools.mso.auto.correct.dlg | Dlg | stub |
| tools.mso.auto.format | Format | stub |
| tools.mso.custom.item | Item | stub |
| tools.mso.insert.equation | Equation | stub |
| tools.mso.insert.symbol | Symbol Insert | stub |
| tools.mso.insert.symbol.dlg | Dlg | stub |
| tools.mso.media.gallery | Gallery | stub |
| tools.names.only | Names Only | stub |
| tools.objects.add.to | Objects Add To | stub |
| tools.objects.convert.to | Objects Convert To | stub |
| tools.objects.remove.from | Objects Remove From | stub |
| tools.ole.objectct.insert | Ole Objectct Insert | stub |
| tools.operations | Operations | stub |
| tools.options.color.palette.dlg | Dlg | stub |
| tools.options.edit.background | Background | stub |
| tools.options.edit.drawing | Drawing | stub |
| tools.options.go.to.drawing | Drawing | stub |
| tools.options.new.page | Page | stub |
| tools.options.page.setup | Setup | stub |
| tools.options.preferences | Preferences | stub |
| tools.options.protect.document | Document | stub |
| tools.options.reorder.pages | Pages | stub |
| tools.org.chart.change.style | Org Chart Change Style | stub |
| tools.org.chart.copy.create | Org Chart Copy Create | stub |
| tools.org.chart.data.compare | Org Chart Data Compare | stub |
| tools.org.chart.data.export | Org Chart Data Export | stub |
| tools.org.chart.data.import | Org Chart Data Import | stub |
| tools.org.chart.fit.to.page | Org Chart Fit To Page | stub |
| tools.org.chart.increase.height | Org Chart Increase Height | stub |
| tools.org.chart.increase.width | Org Chart Increase Width | stub |
| tools.org.chart.layout | Org Chart Layout | stub |
| tools.org.chart.picture.change | Org Chart Picture Change | stub |
| tools.org.chart.picture.delete | Org Chart Picture Delete | stub |
| tools.org.chart.picture.hide | Org Chart Picture Hide | stub |
| tools.org.chart.picture.insert | Org Chart Picture Insert | stub |
| tools.org.chart.picture.insert.multiple | Org Chart Picture Insert Multiple | stub |
| tools.org.chart.picture.insert.one | Org Chart Picture Insert One | stub |
| tools.org.chart.position.type.change | Org Chart Position Type Change | stub |
| tools.org.chart.reduce.height | Org Chart Reduce Height | stub |
| tools.org.chart.reduce.width | Org Chart Reduce Width | stub |
| tools.org.chart.relayout | Org Chart Relayout | stub |
| tools.org.chart.report.move.closer | Org Chart Report Move Closer | stub |
| tools.org.chart.report.move.further | Org Chart Report Move Further | stub |
| tools.org.chart.report.move.left | Org Chart Report Move Left | stub |
| tools.org.chart.report.move.right | Org Chart Report Move Right | stub |
| tools.org.chart.subordinates.expand | Org Chart Subordinates Expand | stub |
| tools.org.chart.subordinates.show.hide | Org Chart Subordinates Show Hide | stub |
| tools.org.chart.synchronize | Org Chart Synchronize | stub |
| tools.outline.weight | Outline Weight | stub |
| tools.pen.add.favorite | Pen Add Favorite | stub |
| tools.pen.pressure | Pen Pressure | stub |
| tools.pencil | Pencil Tool | stub |
| tools.pens | Pens | stub |
| tools.persistent.events | Persistent Events | stub |
| tools.picture.auto.balance | Picture Auto Balance | stub |
| tools.picture.brightness | Picture Brightness | stub |
| tools.picture.compress | Picture Compress | stub |
| tools.picture.contrast | Picture Contrast | stub |
| tools.picture.corrections.dialog | Picture Corrections Dialog | stub |
| tools.picture.insert.from.file | Picture Insert From File | stub |
| tools.pointer.mode.options | Pointer Mode Options | stub |
| tools.pointer_tool.multiple_select | Multiple Select | stub |
| tools.prepare.visio.diagram.for.export.to.flow | Prepare Visio Diagram For Export To Flow | stub |
| tools.quick.connector | Quick Connector | stub |
| tools.recorder.resume | Recorder Resume | stub |
| tools.recorder.stop.visio | Recorder Stop Visio | stub |
| tools.reflection | Reflection | stub |
| tools.reflections.more.options | Reflections More Options | stub |
| tools.refresh.diagram | Refresh Diagram | stub |
| tools.rotate | Rotate Tool | stub |
| tools.rotation_3d | 3 D Rotation | stub |
| tools.rotation_3d_options | 3 D Rotation Options Dialog | stub |
| tools.rules.import.from | Rules Import From | stub |
| tools.rules.to.check | Rules To Check | stub |
| tools.run.vbe | Vbe | stub |
| tools.screen.tip | Screen Tip | stub |
| tools.selection.mode.extend | Extend | stub |
| tools.selection.mode.lasso | Lasso | stub |
| tools.selection.mode.rect | Rect | stub |
| tools.shadow.options.dialog | Shadow Options Dialog | stub |
| tools.share.point.workflow.export | Share Point Workflow Export | stub |
| tools.share.point.workflow.import | Share Point Workflow Import | stub |
| tools.snap_glue_setup | Snap & Glue | stub |
| tools.soft.edges.more.options | Soft Edges More Options | stub |
| tools.spelling | Spelling | stub |
| tools.stencil.close | Stencil Close | stub |
| tools.stencil.explorer | Stencil Explorer | stub |
| tools.stop.ignoring.this.issue | Stop Ignoring This Issue | stub |
| tools.stop.ignoring.validation.rule | Stop Ignoring Rule | stub |
| tools.strikethrough | Strikethrough | stub |
| tools.subprocess.browse.to.other.document | Subprocess Browse To Other Document | stub |
| tools.subprocess.create.from.selection | Subprocess Create From Selection | stub |
| tools.subprocess.create.new | Subprocess Create New | stub |
| tools.subprocess.edit.link | Subprocess Edit Link | stub |
| tools.subprocess.link.to.existing | Subprocess Link To Existing | stub |
| tools.symbols.dialog | Symbols Dialog | stub |
| tools.text_block | Text Block Tool | stub |
| tools.theme.colors.gallery.visio | Theme Colors Gallery Visio | stub |
| tools.theme.connectors | Theme Connectors | stub |
| tools.theme.effects.gallery.visio | Theme Effects Gallery Visio | stub |
| tools.theme.variants | Theme Variants | stub |
| tools.themes.gallery.visio | Themes Gallery Visio | stub |
| tools.themes.gallery.visio.legacy | Themes Gallery Visio Legacy | stub |
| tools.tool.snap.lines | Lines | stub |
| tools.trace.back | Trace Back | stub |
| tools.trace.dependent.cells | Trace Dependent Cells | stub |
| tools.trace.forward | Trace Forward | stub |
| tools.trace.precedent.cells | Trace Precedent Cells | stub |
| tools.translate.options | Options | stub |
| tools.turn.to.next.page | Page | stub |
| tools.turn.to.prev.page | Page | stub |
| tools.usage.permissions | Usage Permissions | stub |
| tools.visio.to.power.point | Visio To Power Point | stub |
| tools.visio.to.word | Visio To Word | stub |
| tools.visual.basic | Visual Basic | stub |
| tools.whats.new.recent.updates | Whats New Recent Updates | stub |
| tools.windows.arrange.all | Windows Arrange All | stub |
| tools.windows.cascade | Windows Cascade | stub |
| tools.writeback.visio.diagram.to.excel.source | Writeback Visio Diagram To Excel Source | stub |
| view.cascade_all | Cascade All | stub |
| view.center.drawing | Drawing | stub |
| view.collapse.shapes.window | Window | stub |
| view.connections | Connections | stub |
| view.custom | Custom | stub |
| view.direction.toggle | Direction | stub |
| view.explorer.active | Active | stub |
| view.explorer.delete | Delete | stub |
| view.explorer.delete_layer | Delete Layer | stub |
| view.explorer.delete_shape | Delete Shape | stub |
| view.explorer.delete_style | Delete Style | stub |
| view.explorer.new_pattern | New Pattern... | stub |
| view.explorer.protect_document | Protect Document... | stub |
| view.explorer.ruler_and_grid | Ruler & Grid... | stub |
| view.explorer.visible | Visible | stub |
| view.first.tile | Tile | stub |
| view.fit.container.to.contents | Fit To Contents | stub |
| view.fit.curve | Curve | stub |
| view.fit.in.window | Window | stub |
| view.full.screen.view | Full Screen View | stub |
| view.full_screen_mode | Full Screen | stub |
| view.go.to.page.toolbar | Toolbar | stub |
| view.go_to.unnamed | Go To | stub |
| view.grid | Show Grid | stub |
| view.guides | Show Guides | stub |
| view.hide.all.toolbars | Toolbars | stub |
| view.hide.document.stencil | Document Stencil | stub |
| view.hide.more.shapes | Shapes | stub |
| view.last | Last | stub |
| view.last.tile | Tile | stub |
| view.left.to.right | Direction Left To Right | stub |
| view.list.insert.after | After | stub |
| view.list.insert.before | Before | stub |
| view.markup | Markup | stub |
| view.master_explorer.page_setup | Page Setup... | stub |
| view.next.comment.markup | Markup | stub |
| view.next.markup | Markup | stub |
| view.next.tile | Tile | stub |
| view.next.window | Window | stub |
| view.page.breaks | Breaks | stub |
| view.pan.object | Object | stub |
| view.pan.zoom | Pan And Zoom Window | stub |
| view.pane.autofit | AutoFit | stub |
| view.pane.autohide | AutoHide | stub |
| view.pane.close | Close | stub |
| view.pane.close_all_stencils | Close All Stencils | stub |
| view.pane.float_window | Float Window | stub |
| view.pane.hide_window | Hide Window | stub |
| view.pane.search_options | Search Options... | stub |
| view.pane.shape_data_sets | Shape Data Sets... | stub |
| view.previous.comment.markup | Markup | stub |
| view.previous.markup | Markup | stub |
| view.previous.tile | Tile | stub |
| view.qat.customize | Customize Quick Access Toolbar | stub |
| view.right.to.left | Left | stub |
| view.rul.sub | Sub | stub |
| view.ruler.grid.dlg | Dlg | stub |
| view.sections | Sections... | stub |
| view.show.comments | Show Comments | stub |
| view.show.formula.tracing.window | Show Formula Tracing Window | stub |
| view.show.ignored.issues.toggle | Show Ignored Issues | stub |
| view.show.line.jumps.toggle | Toggle | stub |
| view.show.page.breaks | Show Page Breaks | stub |
| view.show.shape.sheet.document | Show Shape Sheet Document | stub |
| view.show.shape.sheet.page | Show Shape Sheet Page | stub |
| view.show.shape.sheet.shape | Shape Sheet Show Formulas | stub |
| view.show.tags | Show Tags | stub |
| view.show.training | Show Training | stub |
| view.show_drawing_page | Show Drawing Page... | stub |
| view.ss.window.add.section | Section | stub |
| view.ss.window.change.row.type | Type | stub |
| view.ss.window.collapse | Collapse | stub |
| view.ss.window.deselect | Deselect | stub |
| view.ss.window.expand | Expand | stub |
| view.ss.window.paste.function | Function | stub |
| view.ss.window.paste.name | Name | stub |
| view.ss.window.select | Select | stub |
| view.ss.window.show.section | Section | stub |
| view.ss.window.show.trace.window | Window | stub |
| view.status.bar | Bar | stub |
| view.task.pane | Pane | stub |
| view.task.pane.data.graphic | Graphic | stub |
| view.task.pane.document.management | Management | stub |
| view.task.pane.research | Research | stub |
| view.task.pane.reviewer | Reviewer | stub |
| view.task.panes.menu.visio | Task Panes Menu Visio | stub |
| view.task.toggle.preview.size | Size | stub |
| view.tile_all | Tile All | stub |
| view.toggle.document.stencil | Stencil | stub |
| view.toggle.visio.flow.placeholders | Toggle Visio Flow Placeholders | stub |
| view.toggle_guides | Toggle Guides | stub |
| view.toolbar.action | Action | stub |
| view.toolbar.customize | Customize... | stub |
| view.toolbar.data | Data | stub |
| view.toolbar.developer | Developer | stub |
| view.toolbar.drawing | Drawing | stub |
| view.toolbar.format_text | Format Text | stub |
| view.toolbar.formatting | Formatting | stub |
| view.toolbar.layout_and_routing | Layout & Routing | stub |
| view.toolbar.picture | Picture | stub |
| view.toolbar.reviewing | Reviewing | stub |
| view.toolbar.standard | Standard | stub |
| view.toolbar.stencil | Stencil | stub |
| view.toolbar.view | View | stub |
| view.toolbars.dlg | Dlg | stub |
| view.view100 | View100 | stub |
| view.view150 | View150 | stub |
| view.view200 | View200 | stub |
| view.view400 | View400 | stub |
| view.view50 | View50 | stub |
| view.view75 | View75 | stub |
| view.visual.basic.code | Visual Basic Code | stub |
| view.window.more.windows.dialog | Window More Windows Dialog | stub |
| view.window.new.window | Window New | stub |
| view.window.show.draw.page | Page | stub |
| view.window.show.master.objects | Objects | stub |
| view.window.show.shape.sheet | Show ShapeSheet | stub |
| view.window.switch.windows.menu.word | Window Switch Windows Menu Word | stub |
| view.zoom.area | Area | stub |
| view.zoom.dialog | Zoom Dialog | stub |
| view.zoom.in.ignore.sel | Sel | stub |
| view.zoom.last | Last | stub |
| view.zoom.out.ignore.sel | Sel | stub |
| view.zoom.page.width | Zoom Page Width Visio | stub |
| view.zoom.pt | Pt | stub |
| view.zoom.single.tile | Tile | stub |
| data.link.data.to.shapes | Link Data To Shapes | out-of-scope |
| developer.record.new.macro | Macro Record | out-of-scope |
| file.save.as.macro.drawing | Drawing | out-of-scope |
| tools.com.add.ins.dialog | Com Add Ins Dialog | out-of-scope |
| tools.macro.dlg | Dlg | out-of-scope |
| tools.macro.play | Macro Play | out-of-scope |
| tools.macro.recorder.pause | Macro Recorder Pause | out-of-scope |
| tools.macro.security | Macro Security | out-of-scope |
| tools.pause.recording.macro | Macro | out-of-scope |
| tools.quick.data.link.data.to.shapes | Quick Data Link Data To Shapes | out-of-scope |
| tools.resume.recording.macro | Macro | out-of-scope |
| tools.start.recording.macro | Macro | out-of-scope |
| tools.stop.recording.macro | Macro | out-of-scope |
